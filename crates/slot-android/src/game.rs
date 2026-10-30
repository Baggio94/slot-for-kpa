//! Native libretro mGBA / gpSP game session. All core calls occur on GLSurfaceView's GL thread.
//! Android's SAF bridge materializes a **bounded private cache** copy of each selected ROM.
#[cfg(test)]
#[path = "game_runahead_tests.rs"]
mod runahead_tests;
use crate::pokemon_flash::pokemon_flash_complete_bank_count;
use slot_retro::{ButtonMask, LibretroCore, RetroCore, LinkChannel};
use crate::link_kind::LinkKind;
use crate::link_play::{Frame as LinkFrame, LinkError, LinkPlay};
use crate::multiplayer::Role;
use slot_store::{atomic_write, Core, Platform, StateEntry, StateRing, stamp_now, format_stamp, RING_MAX};
use crate::thumb;
use crate::retroarch_state;
use crate::rewind::{RewindThread, REWIND_BYTES};
use crate::battery_writer::{BatterySnapshot, BatteryWriter};
use std::path::{Path, PathBuf};
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

#[cfg(target_os = "android")]
#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(priority: i32, tag: *const std::ffi::c_char,
        text: *const std::ffi::c_char) -> i32;
}

pub(crate) fn log_launch_timing(message: &str) {
    #[cfg(target_os = "android")]
    if let Ok(text) = std::ffi::CString::new(message) {
        unsafe {
            __android_log_write(4, c"SlotKONKR".as_ptr(), text.as_ptr());
        }
    }
    #[cfg(not(target_os = "android"))]
    eprintln!("SlotKONKR: {message}");
}

// A Link core must NEVER become a standalone emulator after End Link,
// peer loss or Android sleep. The private GBA state is a multiplayer-only
// timeline. Resume requires safe Eject followed by a new solo core launch.
fn emulation_may_advance(link_touched: bool, link_active: bool) -> bool {
    !link_touched || link_active
}

const SRAM_PERIOD: Duration = Duration::from_secs(30);
const REWIND_CAPTURE_FRAMES: u8 = 6; // 10 states/second at the original GBA FPS
const UNDO_WINDOW: Duration = Duration::from_secs(30);

/// Read/reset on the renderer's GL thread only; no extra ROM/core work.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct RunaheadProbe {
    pub calls: u32,
    pub snapshots_us: u64,
    pub speculative_us: u64,
    pub restores_us: u64,
    pub max_us: u64,
    pub failures: u32,
    /// Calls served by the second-instance prediction core.
    pub second_instance_calls: u32,
    /// Preemptive Frames rollbacks (one per button change).
    pub preemptive_rollbacks: u32,
}

/// F8: WALL time spent inside the core and synchronous rewind work or
/// optional battery capture on the GL owner, measured once per real step.
/// Never infer hardware GPU or EGL swap timing from these values.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FrameStepTimings {
    pub core_us: u64,
    pub rewind_work_us: u64,
    pub periodic_battery_us: u64,
}

/// Cartridge header code, not a localized filename: FireRed, LeafGreen,
/// Emerald, Ruby and Sapphire, across original game-code regions.
fn pokemon_gen3_flash_code(code: &[u8; 4]) -> bool {
    [b"BPR", b"BPG", b"BPE", b"AXV", b"AXP"]
        .iter().any(|prefix| code.starts_with(*prefix))
}
/// Drill Dozer has its own cartridge rumble motor. Forcing Game Boy Player
/// rumble for this ROM can interfere with mGBA's one-frame speculative path.
/// Limit the experimental bypass to physical cartridge header V49*, not titles
/// or filenames; gpSP and all other mGBA games retain their current options.
fn drill_dozer_mgba_gbp_bypass(rom: &Path, which: Core, platform: Platform) -> bool {
    if which != Core::Mgba || platform != Platform::Gba { return false; }
    let Ok(mut file) = std::fs::File::open(rom) else { return false; };
    if file.seek(SeekFrom::Start(0xAC)).is_err() { return false; }
    let mut code = [0u8; 4];
    file.read_exact(&mut code).is_ok()
        && code.starts_with(b"V49") && code[3].is_ascii_uppercase()
}

/// The core's own PCM rate, never altered except for absurd values. mGBA
/// emits Game Boy / Color audio at 131072Hz: the old 96kHz ceiling played it
/// 27% slow (and, once the queue filled, chopped) in every pacing mode, and
/// made RETROARCH resample it for the wrong rate. AudioTrack takes up to
/// 192kHz on every Android this app supports (minSdk 31).
pub(crate) fn core_sample_rate(reported: i32) -> i32 {
    reported.clamp(8_000, 192_000)
}

/// Gambatte refuses MBC7 cartridges (Kirby Tilt 'n' Tumble, Command
/// Master): their tilt sensor only runs on mGBA.
pub fn gambatte_cannot_run(rom: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(rom) else { return false; };
    if file.seek(SeekFrom::Start(0x147)).is_err() { return false; }
    let mut cart = [0u8; 1];
    file.read_exact(&mut cart).is_ok() && cart[0] == 0x22
}

/// gpSP emulates no cartridge sensor: Boktai's solar sensor, the tilt
/// of Yoshi Topsy-Turvy and Koro Koro Puzzle, WarioWare: Twisted!'s gyro.
/// These games only run on mGBA.
pub fn gpsp_cannot_run(rom: &Path) -> bool {
    const SENSOR_GAMES: [&[u8; 3]; 6] = [b"U3I", b"U32", b"U33", b"KYG", b"KHP", b"RZW"];
    let Ok(mut file) = std::fs::File::open(rom) else { return false; };
    if file.seek(SeekFrom::Start(0xAC)).is_err() { return false; }
    let mut code = [0u8; 3];
    file.read_exact(&mut code).is_ok() && SENSOR_GAMES.contains(&&code)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Solar { Boktai1, Later }

/// Boktai 1, 2 and 3: cartridges with a solar sensor, which only mGBA
/// emulates.
fn solar_cart(rom: &Path, which: Core, platform: Platform) -> Option<Solar> {
    if which != Core::Mgba || platform != Platform::Gba { return None; }
    let mut file = std::fs::File::open(rom).ok()?;
    file.seek(SeekFrom::Start(0xAC)).ok()?;
    let mut code = [0u8; 4];
    file.read_exact(&mut code).ok()?;
    if code.starts_with(b"U3I") { Some(Solar::Boktai1) }
    else if code.starts_with(b"U32") || code.starts_with(b"U33") { Some(Solar::Later) }
    else { None }
}

fn sma4_runahead_guard(rom: &Path, platform: Platform) -> bool {
    if platform != Platform::Gba { return false; }
    let Ok(mut file) = std::fs::File::open(rom) else { return false; };
    if file.seek(SeekFrom::Start(0xAC)).is_err() { return false; }
    let mut code = [0u8; 4];
    file.read_exact(&mut code).is_ok()
        && code.starts_with(b"AX4") && code[3].is_ascii_uppercase()
}

fn mgba_flash_runahead_guard(rom: &Path, which: Core, platform: Platform) -> bool {
    if which != Core::Mgba || platform != Platform::Gba { return false; }
    let Ok(mut file) = std::fs::File::open(rom) else { return false; };
    if file.seek(SeekFrom::Start(0xAC)).is_err() { return false; }
    let mut code = [0u8; 4];
    file.read_exact(&mut code).is_ok() && pokemon_gen3_flash_code(&code)
}


/** The active protected state generation is part of the SOLO export path.
 * Link must snapshot and compare this very same path; using the legacy
 * {id}.{core}.retroarch-export path falsely detects concurrent writes and
 * ejects Pokémon on End Link after a legitimate cross-core save migration.
 */
fn active_state_generation(root: &Path, id: &str, which: Core) -> Result<Option<String>, String> {
    let record = root.join(format!("{id}.{}.state-generation.active", which.as_str()));
    let key = match std::fs::read_to_string(&record) {
        Ok(key) => key,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot read protected state generation: {e}")),
    };
    if key.len() > 180 || !key.starts_with(&format!("{id}.{}.gen-", which.as_str()))
        || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        || !key.ends_with(|c: char| c.is_ascii_digit()) {
        return Err("Protected state-generation journal is corrupt; refusing stale auto-state".into());
    }
    Ok(Some(key))
}

fn active_export_filename(root: &Path, id: &str, which: Core) -> Result<String, String> {
    Ok(match active_state_generation(root, id, which)? {
        Some(key) => format!("{key}.retroarch-export.state.auto"),
        None => format!("{id}.{}.retroarch-export.state.auto", which.as_str()),
    })
}

fn export_path_matches_snapshot(path: &Path, expected: &str) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(expected)
}

enum PendingUndo {
    Saved { stamp: String, evicted: Option<(String, Vec<u8>, Vec<u8>)> },
    Loaded { prior: Vec<u8> },
}


/// Validated local single-GBA checkpoint recovered from a private Link
/// session. The other player's SRAM and emulator state are NEVER included.
pub struct SoloRejoin {
    pub snapshot: Vec<u8>,
    pub ram: Option<Vec<u8>>,
    pub rtc: Option<Vec<u8>>,
    expected_ram: Option<Vec<u8>>,
    expected_rtc: Option<Vec<u8>>,
    expected_state: Vec<u8>,
    expected_ra_export: Option<Vec<u8>>,
    expected_ra_export_name: String,
}

pub struct GameSession {
    // Wrapped with the native softcore evaluator for opted-in GBA sessions.
    core: Box<dyn RetroCore>,
    // Optional: only used after the real network handshake, never in solo play.
    link: Option<LinkPlay>,
    // Gambatte's Game Link: the core's own serial socket, no private session.
    game_link: Option<crate::gb_netlink::GameLink>,
    // A linked mGBA state may contain TWO cores, incompatible with solo
    // RetroArch .state files. Never overwrite solo timeline after link use.
    link_touched: bool,
    // Isolated mGBA OR gpSP Link core boot. Apart from app-controlled SRAM,
    // libretro's GET_SAVE_DIRECTORY must itself point into this private root.
    // No running solo gpSP may have its save paths redirected in-place.
    linked_boot: Option<Role>,
    // Upstream gpSP serial hardware mode selected before retro_load_game.
    // Changing auto/rfu/mul_poke requires reloading the core, never a
    // cosmetic LinkKind flip after the existing serial port was initialized.
    gpsp_serial: &'static str,
    rom_path: PathBuf,
    rom_stem: String,
    save: PathBuf,
    rtc: PathBuf,
    manual_export_prefix: PathBuf,
    state: PathBuf,
    retroarch_export: PathBuf,
    /// RetroArch's Gambatte predates the Sachen items: export its layout.
    gambatte_legacy_export: bool,
    ring: StateRing,
    rewind: RewindThread,
    rewind_frames: u8,
    frame_count: u32,
    // Presentation-only pixels from speculative frames; save/RA keep the real state.
    runahead_video: Vec<u8>,
    // Preserve the libretro state allocation across speculative frames.
    runahead_state: Vec<u8>,
    runahead_unavailable: bool,
    // A failed rollback after speculative frames poisons the current core.
    // Never advance/save it; require explicit unsaved exit or full reinit.
    runahead_rollback_failed: bool,
    runahead_discard_confirm: bool,
    runahead_probe: RunaheadProbe,
    // Second instance: a private prediction copy of the core, booted on
    // first use. The real core is then never rewound for Run-Ahead.
    runahead_secondary_spec: Option<crate::runahead_secondary::Spec>,
    runahead_secondary: Option<Box<dyn RetroCore>>,
    runahead_secondary_unavailable: bool,
    // Preemptive Frames: states at the start of the last real frames.
    preempt: crate::preemptive::Preempt,
    last_step_timings: FrameStepTimings,
    // Keep mGBA speculative frames away from Pokémon Gen III FLASH1M.
    mgba_flash_runahead_guard: bool,
    sma4_runahead_guard: bool,
    solar: Option<Solar>,
    // The light handed to a solar cart; None while the light sensor decides.
    sun_luminance: Option<u8>,
    palette_live: bool,
    core_kind: Core,
    pending_undo: Option<(Instant, PendingUndo)>,
    last_sram: Instant,
    // F7: owns at most one immutable periodic SRAM/RTC write per solo session.
    battery_writer: Option<BatteryWriter>,
    pub sample_rate: i32,
    pub fps: f64,
}

/// Testable strict recovery used by both mGBA and gpSP after a Link
/// boot failure. Opening the core does not prove its solo save-state loaded.
fn restore_solo_checkpoint_to_core(core: &mut dyn RetroCore,
                                    saved: &[u8]) -> Result<(), String> {
    crate::linked_storage::validate_solo_snapshot(saved)?;
    core.unserialize(saved).map_err(|e|
        format!("Refusing unsafe solo recovery: saved state rejected by core: {e}"))?;
    let verified = core.serialize().map_err(|e|
        format!("Cannot verify restored solo state: {e}"))?;
    crate::linked_storage::validate_solo_snapshot(&verified)
}

/// Preserve original Slot turbo in TURBO mode; in A/B mode Y acts as A
/// and X as B, *in addition to* both original buttons. Applied equally to
/// real and speculative Run-Ahead frames so input prediction never diverges.
fn xy_buttons(buttons: ButtonMask, turbo: bool, frame: u32) -> ButtonMask {
    if turbo { return buttons.turbo(frame); }
    let mut mapped = buttons.without_turbo().0;
    if buttons.0 & ButtonMask::Y != 0 { mapped |= ButtonMask::A; }
    if buttons.0 & ButtonMask::X != 0 { mapped |= ButtonMask::B; }
    ButtonMask(mapped)
}

impl GameSession {
    pub fn open(rom: &Path, core_file: &Path, storage: &Path,
                which: Core, platform: Platform, resume_state: bool,
                rom_stem: &str, gb_palette: Option<&str>,
                colour_correction: bool,
                achievements: Option<&slot_achievements::Service>) -> Result<Self, String> {
        Self::open_internal(rom, core_file, storage, which, platform, resume_state,
            rom_stem, gb_palette, colour_correction, achievements, None, "auto", None, None)
    }

    /// Slot upstream treats GBA Cable/Wireless as a gpSP core option fixed
    /// at ROM load. For link-mode switches, reload the core with the selected
    /// official gpsp_serial option and restore the saved solo timeline.
    pub fn open_gpsp_serial(rom: &Path, core_file: &Path, storage: &Path,
                            rom_stem: &str, colour_correction: bool,
                            achievements: Option<&slot_achievements::Service>,
                            serial: &'static str) -> Result<Self, String> {
        if !matches!(serial, "auto" | "rfu" | "mul_poke" | "mul_aw1" | "mul_aw2") {
            return Err("Unsupported gpSP GBA serial mode".into());
        }
        Self::open_internal(rom, core_file, storage, Core::Gpsp, Platform::Gba,
            true, rom_stem, None, colour_correction, achievements, None, serial, None, None)
    }

    /// A post-Link solo rebuild must not consume an old RetroArch auto-state,
    /// pre-Link checkpoint or one-shot cross-core guard before the live local
    /// GBA snapshot has been verified and durably promoted.
    pub fn open_gpsp_solo_unresumed(rom: &Path, core_file: &Path, storage: &Path,
                                   rom_stem: &str, colour_correction: bool,
                                   achievements: Option<&slot_achievements::Service>,
                                   serial: &'static str) -> Result<Self, String> {
        if !matches!(serial, "auto" | "rfu" | "mul_poke" | "mul_aw1" | "mul_aw2") {
            return Err("Unsupported gpSP solo serial mode".into());
        }
        Self::open_internal(rom, core_file, storage, Core::Gpsp, Platform::Gba,
            false, rom_stem, None, colour_correction, achievements, None, serial, None, None)
    }

    /// Reopen a *fresh* two-console mGBA instance (two GBAs, or two Game
    /// Boys). The libretro link options must be set BEFORE retro_load_game;
    /// an existing solo core cannot switch.
    /// Never pass RetroAchievements to this experimental dual-instance session.
    pub fn open_mgba_linked(rom: &Path, core_file: &Path, storage: &Path,
                            platform: Platform, rom_stem: &str,
                            gb_palette: Option<&str>, colour_correction: bool,
                            role: Role, solo_state: &[u8],
                            peer_rom: Option<&Path>) -> Result<Self, String> {
        // Fail BEFORE any filesystem writes or a dropped solo core if the
        // source state cannot fit as a valid single-console Link seed.
        crate::linked_storage::validate_solo_snapshot(solo_state)?;
        Self::open_internal(rom, core_file, storage, Core::Mgba, platform,
            false, rom_stem, gb_palette, colour_correction, None, Some(role), "auto",
            Some(solo_state), peer_rom)
    }

    /// gpSP requires a NEW libretro instance whose GET_SAVE_DIRECTORY is
    /// private from the moment retro_load_game runs. Merely redirecting
    /// GameSession.save/.rtc on a live solo core does not isolate libretro's
    /// own cached directory pointer or potential core-side battery writes.
    pub fn open_gpsp_linked(rom: &Path, core_file: &Path, storage: &Path,
                            rom_stem: &str, colour_correction: bool,
                            role: Role, solo_state: &[u8],
                            serial: &'static str) -> Result<Self, String> {
        if !matches!(serial, "auto" | "rfu" | "mul_poke" | "mul_aw1" | "mul_aw2") {
            return Err("Unsupported gpSP Link serial mode".into());
        }
        crate::linked_storage::validate_solo_snapshot(solo_state)?;
        Self::open_internal(rom, core_file, storage, Core::Gpsp, Platform::Gba,
            false, rom_stem, None, colour_correction, None, Some(role), serial,
            Some(solo_state), None)
    }

    fn open_internal(rom: &Path, core_file: &Path, storage: &Path,
                     which: Core, platform: Platform, resume_state: bool,
                     rom_stem: &str, gb_palette: Option<&str>,
                     colour_correction: bool,
                     achievements: Option<&slot_achievements::Service>,
                     linked_boot: Option<Role>,
                     gpsp_serial: &'static str,
                     linked_resume: Option<&[u8]>,
                     peer_rom: Option<&Path>) -> Result<Self, String> {
        let opening_at = Instant::now();
        // gpSP is GBA-only; mGBA also links two Game Boys.
        if linked_boot.is_some() && ((platform != Platform::Gba && which != Core::Mgba) ||
            resume_state || achievements.is_some()) ||
            (linked_boot.is_some() != linked_resume.is_some()) {
            return Err("Invalid isolated Multiplayer boot configuration".into());
        }
        if peer_rom.is_some() && (linked_boot.is_none() || which != Core::Mgba) {
            return Err("A remote cartridge is only valid inside mGBA Cable Link".into());
        }
        if let Some(peer) = peer_rom {
            let wanted = if platform == Platform::Gba { "gba" } else { "gb" };
            if !peer.is_file() || peer.extension().and_then(|v|v.to_str()) != Some(wanted)
                || !peer.to_string_lossy().contains("/link-peer-cache/") {
                return Err("Unverified remote ROM cache location".into());
            }
        }
        if !rom.exists() || !core_file.exists() {
            return Err(format!("ROM cache or {} core missing", which.text()));
        }
        let root = storage.join("Saves");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let id = rom.file_stem().and_then(|v| v.to_str())
            .filter(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| "Unsafe ROM cache file name".to_owned())?;
        // Canonical private layout matches RetroArch's per-core filenames.
        // The opaque ROM URI hash is retained for internal cache / state-ring IDs.
        if rom_stem.is_empty() || rom_stem.chars().count() > 200 ||
            rom_stem == "." || rom_stem == ".." ||
            rom_stem.chars().any(|c| c == '/' || c == '\\' || c.is_control()) {
            return Err("Unsafe ROM save filename".into());
        }
        let solo_core_root = root.join(which.text());
        // Every Link attempt starts from live SOLO progress and gets a
        // unique private save timeline. Existing pre-a6 Multiplayer
        // Host/Join files and every earlier session remain untouched.
        let core_root = if let Some(role) = linked_boot {
            let name = if role == Role::Host { "host" } else { "join" };
            crate::linked_storage::fresh_session_for_core(&root, which.text(), id, name)
                .map_err(|e| format!("Cannot allocate private Link session: {e}"))?
        } else { solo_core_root.clone() };
        std::fs::create_dir_all(&core_root).map_err(|e| e.to_string())?;
        let save = core_root.join(format!("{rom_stem}.srm"));
        let rtc = core_root.join(format!("{rom_stem}.rtc"));
        if linked_boot.is_some() {
            // Preserve the exact pre-Link solo SRAM/RTC and state identity in
            // the unique private session. Check the original files have not
            // changed before promoting any Link gameplay to SOLO.
            for (ext, target) in [("srm", &save), ("rtc", &rtc)] {
                let source = solo_core_root.join(format!("{rom_stem}.{ext}"));
                crate::linked_storage::copy_solo_battery(&source, target)
                    .map_err(|e| format!("Cannot seed private Link {ext}: {e}"))?;
                let baseline = core_root.join(format!("{rom_stem}.prelink.{ext}"));
                crate::linked_storage::copy_solo_battery(&source, &baseline)
                    .map_err(|e| format!("Cannot stage Link baseline {ext}: {e}"))?;
            }
            if let Some(snapshot) = linked_resume {
                atomic_write(&core_root.join("prelink.state"), snapshot)
                    .map_err(|e| format!("Cannot stage Link state baseline: {e}"))?;
            }
            // A13r3: compare the generation that SOLO will actually promote,
            // not a stale legacy export abandoned by cross-core migration.
            let ra_name = active_export_filename(&root, id, which)?;
            atomic_write(&core_root.join("prelink.retroarch-export-target.name"),
                ra_name.as_bytes()).map_err(|e|
                format!("Cannot stage Link RetroArch generation identity: {e}"))?;
            let ra_before = root.join(&ra_name);
            log_launch_timing(&format!(
                "Link export generation checkpoint: {}", if ra_name.contains(".gen-") {
                    "protected" } else { "standard" }));
            match std::fs::read(&ra_before) {
                Ok(data) => {
                    if data.is_empty() || data.len() > crate::linked_storage::MAX_SOLO_SNAPSHOT {
                        return Err("Existing RetroArch export exceeds Link rollback limit".into());
                    }
                    atomic_write(&core_root.join("prelink.retroarch-export.state.auto"), &data)
                        .map_err(|e| format!("Cannot stage Link RetroArch baseline: {e}"))?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(format!("Cannot inspect Link RetroArch baseline: {e}")),
            }
        }
        // Save states belong to their specific emulator core. Incompatible
        // mGBA/gpSP states must never be deserialized in the other core.
        // Legacy single-core .state files are still readable by mGBA only.
        let state_root = if linked_boot.is_some() { &core_root } else { &root };
        let mut state = state_root.join(format!("{id}.{}.state", which.as_str()));
        let mut retroarch_export = state_root.join(format!("{id}.{}.retroarch-export.state.auto", which.as_str()));
        // A cross-core SRAM import intentionally skips a possibly older
        // save-state. Its next Save & Eject must write a NEW generation,
        // never overwrite the ignored state or its RetroArch auto export.
        let generation_record = root.join(format!("{id}.{}.state-generation.active", which.as_str()));
        let generation_export_guard = root.join(format!("{id}.{}.state-export-protected", which.as_str()));
        let retroarch_import = state_root.join(format!("{id}.{}.retroarch-import.state.auto", which.as_str()));
        let manual_dir = state_root.join("ManualExports");
        std::fs::create_dir_all(&manual_dir).map_err(|e| e.to_string())?;
        let manual_export_prefix = manual_dir.join(format!("{id}.{}", which.as_str()));
        let ring = StateRing::new(
            if linked_boot.is_some() { core_root.as_path() } else { storage },
            platform, which, id);
        // mGBA's libretro core looks for gba_bios.bin / gb_bios.bin /
        // gbc_bios.bin in RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY.
        // Keep the user-selected SAF folder external and read-only: Android
        // has already staged only allowed BIOS files into this private folder.
        let bios = storage.join("BIOS");
        std::fs::create_dir_all(&bios).map_err(|e| e.to_string())?;
        // Gambatte decides on the boot ROM in retro_init, before the
        // options below are set.
        let init_options = match which {
            Core::Gambatte => vec![("gambatte_gb_bootloader".to_string(), "enabled".to_string())],
            _ => Vec::new(),
        };
        let mut core = LibretroCore::open_with_options(core_file, &bios, &core_root, &init_options)
            .map_err(|e| e.to_string())?;
        // Exact upstream Slot libretro options for colour and Game Boy palettes.
        // Read only the 336-byte header: never read the entire cached ROM.
        let dmg_only = platform != Platform::Gba && which != Core::Gpsp
            && std::fs::File::open(rom).and_then(|mut file| {
                let mut header = [0u8; 0x150];
                file.read_exact(&mut header)?;
                Ok(header[0x143] & 0x80 == 0)
            }).unwrap_or(false);
        let palette_live = dmg_only && gb_palette.is_some();
        let solar = solar_cart(rom, which, platform);
        slot_retro::sensors::set_light_wanted(false);
        slot_retro::sensors::set_tilt_inverted(platform != Platform::Gba);
        match which {
            Core::Mgba => {
                if let Some(role) = linked_boot {
                    core.set_option("mgba_link", "on");
                    core.set_option("mgba_link_player", if role == Role::Host { "0" } else { "1" });
                    if let Some(path) = peer_rom {
                        core.set_option("mgba_link_peer_rom", path.to_str()
                            .ok_or("Remote ROM path is not UTF-8")?);
                    }
                } else {
                    core.set_option("mgba_link", "off");
                }
                core.set_option("mgba_use_bios", "ON");
                core.set_option("mgba_skip_bios", "OFF");
                core.set_option("mgba_sgb_borders", "OFF");
                // Drill Dozer's built-in rumble is not Game Boy Player
                // hardware. Keep normal mGBA behavior for all other ROMs.
                let force_gbp = !drill_dozer_mgba_gbp_bypass(rom, which, platform);
                core.set_option("mgba_force_gbp", if force_gbp { "ON" } else { "OFF" });
                if !force_gbp {
                    log_launch_timing("Drill Dozer mGBA: GBP rumble OFF; Run-Ahead remains enabled for testing");
                }
                if let Some(name) = gb_palette.filter(|_| dmg_only) {
                    core.set_option("mgba_gb_model", "Game Boy");
                    core.set_option("mgba_gb_colors_preset", "0");
                    core.set_option("mgba_gb_colors", name);
                } else {
                    core.set_option("mgba_gb_model", "Autodetect");
                    core.set_option("mgba_gb_colors_preset", "1");
                    core.set_option("mgba_gb_colors", "GBC Dark Green →A");
                }
                core.set_option("mgba_color_correction",
                    if colour_correction { "Auto" } else { "OFF" });
            }
            Core::Gpsp => {
                // Replicate official Slot/fork's core.rs: this option must
                // precede core.load(), and reloading is necessary to switch.
                core.set_option("gpsp_serial", gpsp_serial);
                core.set_option("gpsp_bios", "auto");
                core.set_option("gpsp_boot_mode", "bios");
                core.set_option("gpsp_color_correction",
                    if colour_correction { "enabled" } else { "disabled" });
            }
            Core::Gambatte => {
                // Same choices as mGBA: the GB/GBC boot ROM when the user
                // supplied one, GBC colours for DMG carts unless a Slot
                // palette is picked, and GBC colour correction on request.
                core.set_option("gambatte_gb_hwmode", "Auto");
                core.set_option("gambatte_gb_bootloader", "enabled");
                core.set_option("gambatte_up_down_allowed", "disabled");
                // RetroArch's default. Without a value Gambatte reads 0
                // and rumble carts (Pokémon Pinball) never vibrate.
                core.set_option("gambatte_rumble_level", "10");
                match gb_palette.and_then(crate::gb_core_switch::gambatte_palette) {
                    Some(name) => {
                        core.set_option("gambatte_gb_colorization", "internal");
                        core.set_option("gambatte_gb_internal_palette", name);
                    }
                    None => core.set_option("gambatte_gb_colorization", "GBC"),
                }
                core.set_option("gambatte_gbc_color_correction",
                    if colour_correction { "GBC only" } else { "disabled" });
            }
        }
        // rcheevos shares ONE evaluator, protected against cross-console
        // memory reads. GB=4 / GBA=5 / GBC=6 use their own libretro maps.
        // Speculative frames never reach it (including GB/GBC Run-Ahead).
        let console = match platform {
            Platform::Gb => 4, Platform::Gba => 5, Platform::Gbc => 6,
        };
        // Second-instance Run-Ahead boots its copy from these, solo only.
        let runahead_secondary_spec = linked_boot.is_none().then(|| crate::runahead_secondary::Spec {
            core_file: core_file.to_path_buf(),
            system_dir: bios.clone(),
            scratch_dir: storage.join("RunAheadInstance"),
            rom: rom.to_path_buf(),
            options: core.options(),
            primary_id: core.instance_id(),
        });
        let mut core: Box<dyn RetroCore> = match achievements {
            Some(service) => service.wrap_for_console(Box::new(core), console),
            None => Box::new(core),
        };
        core.load(rom).map_err(|e| e.to_string())?;
        let load_ms = opening_at.elapsed().as_millis();
        let restore_started = Instant::now();
        // With Share Saves, a GB/GBC cart switched between mGBA and Gambatte
        // brings its newer battery along; that core's older state must not resume.
        let carried_battery = linked_boot.is_none() && crate::gb_core_switch::sharing() && match
            crate::gb_core_switch::carry_battery(&root, rom_stem, which, platform) {
            Ok(carried) => carried,
            Err(error) => {
                eprintln!("slot-konkr: battery carry from the other core skipped: {error}");
                false
            }
        };
        if carried_battery {
            log_launch_timing(&format!(
                "{}: newer battery from the other GB core, save state not resumed", which.text()));
        }
        let ram = std::fs::read(&save).or_else(|err| {
            if linked_boot.is_none() && err.kind() == std::io::ErrorKind::NotFound {
                // Migrate the pre-0.0.7 hashed cache non-destructively.
                std::fs::read(root.join(format!("{id}.{}.srm", which.as_str())))
                    .or_else(|_| {
                        if which == Core::Mgba {
                            std::fs::read(root.join(format!("{id}.srm")))
                        } else { Err(err) }
                    })
            } else { Err(err) }
        });
        if let Ok(ram) = ram {
            if let Err(error) = core.load_save_ram(&ram) {
                // Never continue with blank RAM and subsequently export it
                // over a verified shared save from the OTHER emulator.
                let marker = root.join(format!("{id}.{}.skip-resume.once", which.as_str()));
                if linked_boot.is_some() || marker.is_file() {
                    // A fresh Link session must never continue with blank
                    // battery data when the exact solo SRAM copy was refused.
                    // Abandon this isolated attempt and restore the solo core.
                    return Err(format!("Core rejected required GBA SRAM; originals preserved: {error}"));
                }
                eprintln!("slot-konkr: SRAM restore skipped: {error}");
            }
        }
        if let Ok(bytes) = std::fs::read(&rtc) {
            if let Err(error) = core.load_rtc(&bytes) {
                eprintln!("slot-konkr: RTC restore skipped: {error}");
            }
        }
        // Both Link cores start from this player's exact live SOLO snapshot
        // and private SRAM/RTC. mGBA wraps it as a two-GBA SLK1 container;
        // gpSP restores the original single-GBA state in a NEW sandboxed
        // core with the desired serial mode already set before ROM load.
        // Neither may silently restart at the title screen on a bad state.
        if let Some(solo) = linked_resume {
            if which == Core::Mgba {
                let combined = if peer_rom.is_some() {
                    let fresh = core.serialize().map_err(|e|
                        format!("Cannot capture the two different GBA cartridges: {e}"))?;
                    let player = if linked_boot == Some(Role::Host) { 0 } else { 1 };
                    crate::link_state::with_local_player(&fresh, player, solo)
                        .ok_or("Cannot restore own state without modifying peer cartridge")?
                } else { crate::link_state::pair(solo) };
                core.unserialize(&combined).map_err(|error|
                    format!("Cannot restore live solo game into Link engine: {error}"))?;
                let verified = core.serialize().map_err(|error|
                    format!("Cannot verify new SLK1 link session: {error}"))?;
                if !crate::link_state::is_pair(&verified) {
                    return Err("mGBA Link restored without two-GBA SLK1 state".into());
                }
                log_launch_timing(if peer_rom.is_some() {
                    "mGBA Link: distinct ROMs, own solo state only; private peer kept fresh"
                } else {
                    "mGBA Link: loaded live solo progress into isolated SLK1 pair"
                });
            } else {
                core.unserialize(solo).map_err(|error|
                    format!("Cannot restore live gpSP progress into private Link core: {error}"))?;
                let verified = core.serialize().map_err(|error|
                    format!("Cannot verify private gpSP Link restore: {error}"))?;
                crate::linked_storage::validate_solo_snapshot(&verified)?;
                log_launch_timing("gpSP Link: loaded solo progress into private libretro save directory");
            }
        }
        // Switching GBA emulation cores can import a newer battery save from
        // the other core. A stale same-core .state.auto would roll progress
        // backwards despite correct SRAM syncing. Kotlin stages a one-shot
        // marker ONLY when the imported save comes from the other core.
        let skip_resume_marker = root.join(format!("{id}.{}.skip-resume.once", which.as_str()));
        // Link sessions never consume a pending solo mGBA/gpSP
        // cross-core guard; it belongs exclusively to solo resume.
        let cross_core_import = linked_boot.is_none() && skip_resume_marker.exists();
        if cross_core_import {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH)
                .map_err(|e| format!("Invalid system clock for state generation: {e}"))?
                .as_nanos();
            let key = format!("{id}.{}.gen-{nonce}", which.as_str());
            state = root.join(format!("{key}.state"));
            retroarch_export = root.join(format!("{key}.retroarch-export.state.auto"));
            // Fail closed BEFORE consuming the legacy one-shot guard: if
            // storage is unavailable the user cannot accidentally overwrite
            // old states or resume the wrong historical timeline.
            atomic_write(&generation_export_guard, b"preserve previous auto-state")
                .map_err(|e| format!("Cannot guard existing RetroArch states: {e}"))?;
            atomic_write(&generation_record, key.as_bytes())
                .map_err(|e| format!("Cannot record protected save-state generation: {e}"))?;
            std::fs::remove_file(&skip_resume_marker)
                .map_err(|e| format!("Cannot clear cross-core resume guard: {e}"))?;
            log_launch_timing("Cross-core SRAM selected: new immutable state generation; old auto states protected");
        } else if linked_boot.is_none() {
            if let Some(key) = active_state_generation(&root, id, which)? {
                state = root.join(format!("{key}.state"));
                retroarch_export = root.join(format!("{key}.retroarch-export.state.auto"));
            }
        }
        let gambatte_legacy_export = which == Core::Gambatte && std::fs::read(&retroarch_import)
            .ok().and_then(|bytes| retroarch_state::decode(&bytes).ok())
            .is_some_and(|raw| crate::gambatte_state::lacks_sachen(&raw));
        if resume_state && linked_boot.is_none() && !cross_core_import && !carried_battery {
            // Prefer a successfully decoded RetroArch auto state, then the
            // Slot private state. A compressed/unrecognized external file is
            // kept intact and is NEVER interpreted as raw emulator memory.
            let external = std::fs::read(&retroarch_import)
                .ok().and_then(|bytes| match retroarch_state::decode(&bytes) {
                    // An older RetroArch Gambatte's state gets today's layout.
                    Ok(raw) if gambatte_legacy_export =>
                        Some(crate::gambatte_state::add_sachen(&raw).unwrap_or(raw)),
                    Ok(raw) => Some(raw),
                    Err(error) => {
                        eprintln!("slot-konkr: external {} savestate skipped: {error}", which.text());
                        None
                    }
                });
            let local = || std::fs::read(&state).or_else(|err| {
                if which == Core::Mgba && err.kind() == std::io::ErrorKind::NotFound {
                    std::fs::read(root.join(format!("{id}.state")))
                } else {
                    Err(err)
                }
            }).ok();
            // Re-entering solo after mGBA Link must resume the *fresh*
            // solo snapshot preserved before switching cores. Older
            // RetroArch imports must not silently roll back that timeline.
            let marker = state.with_extension("prefer-solo-after-link.once");
            let prefer_private = marker.is_file();
            let (chosen, used_private) = if state != root.join(format!("{id}.{}.state", which.as_str())) {
                // A state deliberately skipped during SharedGBA migration
                // cannot reappear through an old external RetroArch auto.
                (std::fs::read(&state).ok(), false)
            } else if prefer_private {
                match local() {
                    Some(bytes) => (Some(bytes), true),
                    None => (external, false),
                }
            } else { (external.or_else(local), false) };
            if let Some(bytes) = chosen {
                match core.unserialize(&bytes) {
                    Ok(()) if used_private => {
                        std::fs::remove_file(&marker).map_err(|e|
                            format!("Cannot clear one-shot solo-state guard: {e}"))?;
                    }
                    Ok(()) => {}
                    Err(error) => {
                        eprintln!("slot-konkr: {} state resume skipped: {error}", which.text());
                    }
                }
            }
        }
        log_launch_timing(&format!(
            "Core init + ROM: {load_ms}ms, SRAM/RTC/state restore: {}ms, resume={resume_state}, total: {}ms",
            restore_started.elapsed().as_millis(), opening_at.elapsed().as_millis()
        ));
        let sample_rate = core.av_info().sample_rate.round() as i32;
        let fps = core.av_info().fps.clamp(30.0, 120.0);
        let flash_guard = mgba_flash_runahead_guard(rom, which, platform);
        let sma4_guard = sma4_runahead_guard(rom, platform);
        if flash_guard && linked_boot.is_none() {
            log_launch_timing("mGBA Flash safety: Run-Ahead OFF for Pokemon Gen III, including after End Link; gpSP unchanged");
        }
        if sma4_guard {
            log_launch_timing("Super Mario Advance 4: Run-Ahead OFF for mGBA/gpSP to protect SRAM");
        }
        // A worker only owns immutable copies of SOLO battery files. Private
        // Link sessions retain their existing synchronous checkpointing.
        // Thread startup failure safely falls back to the old GL writes.
        let battery_writer = if linked_boot.is_none() {
            match BatteryWriter::spawn() {
                Ok(worker) => Some(worker),
                Err(error) => {
                    log_launch_timing(&format!(
                        "F7 async battery unavailable; using synchronous writes: {error}"));
                    None
                }
            }
        } else { None };
        let mut session = Self {
            core, link: None, game_link: None, link_touched: false, linked_boot, gpsp_serial,
            rom_path: rom.to_path_buf(), rom_stem: rom_stem.to_owned(),
            save, rtc, manual_export_prefix, state, retroarch_export,
            gambatte_legacy_export, ring,
            rewind: RewindThread::spawn(REWIND_BYTES), rewind_frames: 0,
            frame_count: 0, palette_live, core_kind: which,
            runahead_video: Vec::new(), runahead_state: Vec::new(),
            runahead_unavailable: false,
            runahead_rollback_failed: false,
            runahead_discard_confirm: false,
            runahead_probe: RunaheadProbe::default(),
            runahead_secondary_spec, runahead_secondary: None,
            runahead_secondary_unavailable: false,
            preempt: crate::preemptive::Preempt::default(),
            last_step_timings: FrameStepTimings::default(),
            mgba_flash_runahead_guard: flash_guard,
            sma4_runahead_guard: sma4_guard,
            solar, sun_luminance: None,
            pending_undo: None, last_sram: Instant::now(), battery_writer,
            sample_rate: core_sample_rate(sample_rate),
            fps,
        };
        // The handheld's default sun until the runtime applies this game's own.
        let light = slot_retro::sensors::available() & slot_retro::sensors::ILLUMINANCE != 0;
        session.set_sun(crate::emulation::default_sun(light));
        Ok(session)
    }

    /// False while cable lockstep is awaiting the other player. The caller
    /// must not subtract an emulated-frame budget until this returns true.
    pub fn advance(&mut self, buttons: u16, turbo: bool, record_rewind: bool) -> bool {
        self.last_step_timings = FrameStepTimings::default();
        if self.runahead_rollback_failed { return false; }
        // A suspended or ended Multiplayer session must not fall through
        // to core.run_frame() merely because self.link became None. This
        // also handles Android onPause outside the Multiplayer overlay.
        if !emulation_may_advance(self.link_touched, self.link.is_some()) {
            return false;
        }
        if self.game_link.as_ref().is_some_and(|link| !link.may_run()) { return false; }
        self.runahead_video.clear();
        self.follow_light_sensor();
        if self.link.is_none() && !self.preempt_before_frame(buttons, turbo) { return false; }
        let pressed = ButtonMask(buttons);
        let mapped = xy_buttons(pressed, turbo, self.frame_count);
        let core_started = Instant::now();
        let advanced = if let Some(link) = self.link.as_mut() {
            matches!(link.frame(self.core.as_mut(), mapped), LinkFrame::Advanced)
        } else {
            self.core.run_frame(mapped);
            true
        };
        self.last_step_timings.core_us = core_started.elapsed().as_micros() as u64;
        if !advanced { return false; }
        self.frame_count = self.frame_count.wrapping_add(1);
        if record_rewind && !self.in_multiplayer() {
            self.rewind_frames += 1;
            if self.rewind_frames >= REWIND_CAPTURE_FRAMES {
                self.rewind_frames = 0;
                // Original XOR/LZ4 history; never export it to RetroArch.
                let capture_started = Instant::now();
                if let Ok(snapshot) = self.core.serialize() {
                    self.rewind.push(snapshot);
                }
                self.last_step_timings.rewind_work_us =
                    capture_started.elapsed().as_micros() as u64;
            }
        } else { self.rewind_frames = 0; }
        if self.last_sram.elapsed() >= SRAM_PERIOD {
            let battery_started = Instant::now();
            self.save_sram();
            self.last_step_timings.periodic_battery_us =
                battery_started.elapsed().as_micros() as u64;
        }
        true
    }

    pub(crate) fn last_step_timings(&self) -> FrameStepTimings {
        self.last_step_timings
    }

    /// The pinned mGBA libretro core is SINGLE-PLAYER (it ignores controller
    /// port 1 and never registers a netpacket interface). gpSP is the ONLY
    /// installed core with genuine GBA serial/RFU netpacket callbacks. Both
    /// Cable and Wireless therefore use gpSP's network interface, never the
    /// simulated two-input single-core path used by original Slot tests.
    pub fn begin_multiplayer(&mut self, role: Role, mode: LinkKind,
                             transport: Box<dyn LinkChannel>) -> Result<(), String> {
        if self.link.is_some() { return Err("Already linked".into()); }
        if self.link_touched {
            return Err("Relaunch cartridge before another link".into());
        }
        // Both cores are booted in immutable fresh private Link sessions.
        // For gpSP that includes libretro's GET_SAVE_DIRECTORY, not only
        // Slot's own SRAM/RTC writers. Refuse an in-place solo gpSP Link.
        let Some(boot_role) = self.linked_boot else {
            return Err("Reopen the core in a private Link session before connecting".into());
        };
        if boot_role != role {
            return Err("Private Link session role does not match network role".into());
        }
        match self.core_kind {
            Core::Mgba => {
                if mode != LinkKind::Cable || self.core.supports_netpacket() {
                    return Err("mGBA Link requires Cable lockstep".into());
                }
                let state = self.core.serialize().map_err(|e| e.to_string())?;
                if !crate::link_state::is_pair(&state) {
                    return Err("mGBA Link core did not initialize an SLK1 two-GBA state".into());
                }
            }
            Core::Gpsp => {
                if !self.core.supports_netpacket() {
                    return Err("gpSP did not register GBA serial/RFU netpacket callbacks".into());
                }
            }
            Core::Gambatte => return Err("Gambatte has no Slot Link support".into()),
        }
        let link = LinkPlay::begin(self.core.as_mut(), role, mode, transport)?;
        self.link = Some(link);
        self.link_touched = true;
        self.runahead_video.clear();
        self.preempt.clear();
        self.rewind_frames = 0;
        self.pending_undo = None;
        Ok(())
    }

    /// Gambatte keeps playing its own solo game; Game Link only plugs the
    /// core's serial port into the other console. Run-Ahead, rewind and
    /// states stay off meanwhile: a replayed frame would resend its bytes.
    pub fn begin_game_link(&mut self, role: Role, host: std::net::Ipv4Addr,
                           transport: Box<dyn LinkChannel>) -> Result<(), String> {
        if self.core_kind != Core::Gambatte { return Err("Game Link needs Gambatte".into()); }
        if self.in_multiplayer() || self.link.is_some() { return Err("Already linked".into()); }
        if self.runahead_rollback_failed { return Err("Unsafe Run-Ahead state; Link blocked".into()); }
        self.game_link = Some(crate::gb_netlink::GameLink::begin(
            self.core.as_mut(), role, host, transport));
        self.runahead_video.clear();
        self.preempt.clear();
        self.rewind_frames = 0;
        self.pending_undo = None;
        Ok(())
    }

    fn in_multiplayer(&self) -> bool {
        self.link_touched || self.linked_boot.is_some() || self.game_link.is_some()
    }

    pub fn link_fault(&self) -> Option<LinkError> {
        self.link.as_ref().and_then(|link| link.fault())
            .or_else(|| self.game_link.as_ref().and_then(|link| link.fault()))
    }

    pub fn linked(&self) -> bool { self.link.is_some() || self.game_link.is_some() }

    /// Keep the connected gameplay session alive while a native overlay
    /// is open. The peer stops at its next emulation callback without
    /// sending any artificial GBA serial packets.
    pub fn set_link_paused(&mut self, paused: bool) {
        if let Some(link) = self.link.as_mut() { link.set_local_paused(paused); }
        if let Some(link) = self.game_link.as_mut() { link.set_local_paused(paused); }
    }

    /// An ended link must never become a local solo timeline. Eject and
    /// reopen the ROM to reinitialize both save and emulation state safely.
    pub fn requires_relaunch(&self) -> bool {
        self.link_touched && self.link.is_none()
    }

    pub fn core_kind(&self) -> Core { self.core_kind }
    pub fn loaded_serial(&self) -> &'static str { self.gpsp_serial }
    pub fn rom_path(&self) -> &Path { &self.rom_path }
    pub fn rom_stem(&self) -> &str { &self.rom_stem }
    pub fn is_link_sandbox(&self) -> bool { self.linked_boot.is_some() || self.link_touched }

    /// After an attempted Link boot fails, the newly loaded solo core may
    /// have silently ignored an old/unreadable state during open_internal().
    /// Require an explicit successful deserialize of the live pre-Link bytes
    /// before calling the solo timeline recovered. No ROM title reset, no
    /// fallback to an arbitrary external RetroArch auto-state.
    pub fn restore_checkpoint_after_link_failure(&mut self,
                                                  saved: &[u8]) -> Result<(), String> {
        if self.linked_boot.is_some() || self.link_touched {
            return Err("Cannot restore a solo checkpoint inside a Link session".into());
        }
        // A previous real-frame battery copy cannot land after recovery.
        self.flush_periodic_battery();
        restore_solo_checkpoint_to_core(self.core.as_mut(), saved)?;
        // Clear presentation/input time travel from any pre-recovery state.
        self.runahead_video.clear();
        self.preempt.clear();
        self.rewind_frames = 0;
        self.pending_undo = None;
        Ok(())
    }

    /// Save the solo state before replacing the entire libretro core with
    /// the linked dual-GBA engine. Abort rather than discard solo progress.
    pub fn preserve_solo_for_link(&mut self) -> Result<Vec<u8>, String> {
        if self.runahead_rollback_failed { return Err("Unsafe Run-Ahead state; Link checkpoint blocked".into()); }
        if self.linked_boot.is_some() || self.link_touched {
            return Err("Only a clean solo game can start mGBA Link".into());
        }
        // A periodic save must not complete after this Link checkpoint and
        // replace the fresh solo state with an earlier battery generation.
        self.flush_periodic_battery();
        // A13: never seed a Pokémon Cable session from a malformed FLASH1M
        // image. The in-memory core can differ from the verified disk save,
        // so check it BEFORE writing the checkpoint or dropping the solo core.
        let battery = self.core.save_ram_verified()
            .map_err(|e| format!("Cannot read active battery before Link: {e}"))?;
        self.ensure_pokemon_flash(battery.as_deref(), "before Link")?;
        let bytes = self.core.serialize().map_err(|e| e.to_string())?;
        crate::linked_storage::validate_solo_snapshot(&bytes)?;
        atomic_write(&self.state, &bytes).map_err(|e| format!("Cannot preserve solo state: {e}"))?;
        if let Some(data) = battery {
            atomic_write(&self.save, &data).map_err(|e| format!("Cannot preserve solo SRAM: {e}"))?;
        }
        if let Some(data) = self.core.save_rtc() {
            atomic_write(&self.rtc, &data).map_err(|e| format!("Cannot preserve solo RTC: {e}"))?;
        }
        // One-time recovery preference. It leaves the RetroArch import file
        // intact and is cleared only after successful private state restore.
        atomic_write(&self.state.with_extension("prefer-solo-after-link.once"), b"1")
            .map_err(|e| format!("Cannot record solo recovery guard: {e}"))?;
        Ok(bytes)
    }

    /// Capture the player's own live GBA BEFORE the transport or core is
    /// destroyed. mGBA's SLK1 container has both consoles; only the local
    /// role may be extracted. gpSP already has one local GBA.
    fn ensure_pokemon_flash(&self, ram: Option<&[u8]>, stage: &str) -> Result<(), String> {
        if !self.mgba_flash_runahead_guard { return Ok(()); }
        let Some(bytes) = ram else {
            return Err(format!("Pokémon FLASH1M unavailable {stage}; solo save protected"));
        };
        if pokemon_flash_complete_bank_count(bytes) == 0 {
            log_launch_timing(&format!("Pokémon FLASH1M rejected {stage}: no complete checksum-valid save bank"));
            return Err(format!("Pokémon FLASH1M invalid {stage}; solo save protected"));
        }
        Ok(())
    }

    pub fn prepare_solo_rejoin(&mut self) -> Result<SoloRejoin, String> {
        let role = self.linked_boot.ok_or("Not a private multiplayer game")?;
        if !self.link_touched { return Err("Link was never started".into()); }
        let full = self.core.serialize().map_err(|e| format!("Cannot capture Link progress: {e}"))?;
        let snapshot = if self.core_kind == Core::Mgba {
            let player = if role == Role::Host { 0 } else { 1 };
            crate::link_state::local(&full, player)
                .ok_or("Cannot extract local mGBA from SLK1 pair")?
        } else { full };
        crate::linked_storage::validate_solo_snapshot(&snapshot)?;
        let ram = self.core.save_ram_verified()
            .map_err(|e| format!("Cannot read active Link battery: {e}"))?;
        // A13: a corrupt local Link player must never become the solo save,
        // even when its SLK1 snapshot itself deserializes correctly.
        self.ensure_pokemon_flash(ram.as_deref(), "after Link")?;
        let rtc = self.core.save_rtc();
        let root = self.save.parent().ok_or("No private Link save root")?;
        let baseline = |name: &str| -> Result<Option<Vec<u8>>, String> {
            match std::fs::read(root.join(name)) {
                Ok(data) => Ok(Some(data)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("Cannot read Link baseline {name}: {e}")),
            }
        };
        let expected_ram = baseline(&format!("{}.prelink.srm", self.rom_stem))?;
        let expected_rtc = baseline(&format!("{}.prelink.rtc", self.rom_stem))?;
        let expected_state = std::fs::read(root.join("prelink.state"))
            .map_err(|e| format!("Missing pre-Link solo checkpoint: {e}"))?;
        crate::linked_storage::validate_solo_snapshot(&expected_state)?;
        let expected_ra_export = baseline("prelink.retroarch-export.state.auto")?;
        let expected_ra_export_name = std::fs::read_to_string(
            root.join("prelink.retroarch-export-target.name"))
            .map_err(|e| format!("Missing pre-Link RetroArch export identity: {e}"))?;
        if expected_ra_export_name.len() > 220 ||
            !expected_ra_export_name.ends_with(".retroarch-export.state.auto") ||
            !expected_ra_export_name.bytes().all(|b|
                b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
            return Err("Invalid pre-Link RetroArch generation identity".into());
        }
        // Always preserve the final native local snapshot in the isolated
        // private history even if a later solo recovery fails.
        atomic_write(&root.join("last-local-gba.state"), &snapshot)
            .map_err(|e| format!("Cannot preserve recovered Link state: {e}"))?;
        if let Some(ref bytes) = ram {
            atomic_write(&self.save, bytes)
                .map_err(|e| format!("Cannot preserve recovered Link SRAM: {e}"))?;
        }
        if let Some(ref bytes) = rtc {
            atomic_write(&self.rtc, bytes)
                .map_err(|e| format!("Cannot preserve recovered Link RTC: {e}"))?;
        }
        Ok(SoloRejoin { snapshot, ram, rtc, expected_ram, expected_rtc,
            expected_state, expected_ra_export, expected_ra_export_name })
    }

    /// The rebuilt core must be an ordinary single-GBA SOLO context.
    /// No link-specific state, netpacket callback, RA evaluator or network
    /// transport is carried into it. Verify the exact live checkpoint.
    pub fn restore_local_after_link(&mut self, rejoin: &SoloRejoin) -> Result<(), String> {
        if self.linked_boot.is_some() || self.link_touched || self.link.is_some() {
            return Err("Refusing to resume Link data in non-solo core".into());
        }
        self.ensure_pokemon_flash(rejoin.ram.as_deref(), "while restoring solo")?;
        restore_solo_checkpoint_to_core(self.core.as_mut(), &rejoin.snapshot)?;
        if let Some(ram) = rejoin.ram.as_deref() {
            self.core.load_save_ram(ram)
                .map_err(|e| format!("Core rejected local Link SRAM: {e}"))?;
        }
        if let Some(rtc) = rejoin.rtc.as_deref() {
            self.core.load_rtc(rtc)
                .map_err(|e| format!("Core rejected local Link RTC: {e}"))?;
        }
        self.runahead_video.clear();
        self.preempt.clear();
        self.rewind_frames = 0;
        self.pending_undo = None;
        Ok(())
    }

    /// Atomically switch the saved SOLO progress to its just-detached local
    /// GBA only when its pre-Link baseline is still untouched. Immutable
    /// before/after backup generations survive a crash or future conflict.
    pub fn commit_rejoined_solo(&mut self, rejoin: &SoloRejoin)
        -> Result<std::path::PathBuf, String> {
        if self.linked_boot.is_some() || self.link_touched || self.link.is_some() {
            return Err("A Link sandbox cannot promote itself to SOLO".into());
        }
        // If another worker really changed the active generation, still
        // refuse all publication. Only the false legacy-vs-generation
        // mismatch is fixed. This runs before ANY promotion writes.
        if !export_path_matches_snapshot(&self.retroarch_export,
            &rejoin.expected_ra_export_name) {
            return Err("Active RetroArch save generation changed during Link; original solo save protected".into());
        }
        // No publication to SOLO unless the new core has the exact local
        // FLASH and at least one complete generation after the transition.
        let battery = self.core.save_ram_verified()
            .map_err(|e| format!("Cannot verify active battery before promotion: {e}"))?;
        self.ensure_pokemon_flash(battery.as_deref(), "before promotion")?;
        let resumed = self.core.serialize()
            .map_err(|e| format!("Cannot verify resumed solo state: {e}"))?;
        crate::linked_storage::validate_solo_snapshot(&resumed)?;
        if let Some(expected) = rejoin.ram.as_deref() {
            if battery.as_deref() != Some(expected) {
                return Err("Recovered solo SRAM differs from Link original; refusing promotion".into());
            }
        }
        let compressed = retroarch_state::encode(&resumed)
            .map_err(|e| format!("Cannot encode resumed RetroArch state: {e}"))?;
        let backup_root = self.save.parent()
            .and_then(|d| d.parent())
            .ok_or("Missing Slot Saves root")?
            .join("LinkPromotions");
        let promotions = [
            crate::linked_storage::PromotionFile {
                path: self.save.as_path(), expected: rejoin.expected_ram.as_deref(),
                after: rejoin.ram.as_deref(),
            },
            crate::linked_storage::PromotionFile {
                path: self.rtc.as_path(), expected: rejoin.expected_rtc.as_deref(),
                after: rejoin.rtc.as_deref(),
            },
            crate::linked_storage::PromotionFile {
                path: self.state.as_path(), expected: Some(&rejoin.expected_state),
                after: Some(&resumed),
            },
            crate::linked_storage::PromotionFile {
                path: self.retroarch_export.as_path(),
                expected: rejoin.expected_ra_export.as_deref(),
                after: Some(&compressed),
            },
        ];
        let backup = crate::linked_storage::promote_player_files(&backup_root, &promotions)?;
        // A stale RetroArch external auto-state might still exist. Always
        // prefer newly promoted private progress at least on the next launch.
        atomic_write(&self.state.with_extension("prefer-solo-after-link.once"), b"1")
            .map_err(|e| format!("Link progress committed but resume guard failed: {e}"))?;
        self.last_sram = Instant::now();
        Ok(backup)
    }

    pub fn wireless_capable(&self) -> bool {
        self.core_kind == Core::Gpsp && self.core.supports_netpacket()
    }

    pub fn end_multiplayer(&mut self) {
        if let Some(mut link) = self.link.take() {
            link.end(self.core.as_mut());
        }
        if let Some(mut link) = self.game_link.take() {
            link.end(self.core.as_mut());
        }
        self.runahead_video.clear();
        self.preempt.clear();
        self.rewind_frames = 0;
    }

    pub fn mgba_flash_runahead_blocked(&self) -> bool { self.mgba_flash_runahead_guard }
    pub fn sma4_runahead_blocked(&self) -> bool { self.sma4_runahead_guard }

    /// True only after prediction has mutated the core and full restoration
    /// failed. Treat all save paths as unsafe until a fresh session is loaded.
    pub fn runahead_rollback_failed(&self) -> bool { self.runahead_rollback_failed }

    /// Require two deliberate Exit actions before discarding a poisoned
    /// in-memory session. Do not touch existing on-disk checkpoints.
    pub fn confirm_rollback_discard(&mut self) -> bool {
        if !self.runahead_rollback_failed { return false; }
        if self.runahead_discard_confirm { true }
        else { self.runahead_discard_confirm = true; false }
    }

    pub fn runahead_supported(&self) -> bool {
        !self.mgba_flash_runahead_guard && !self.sma4_runahead_guard
            && !self.in_multiplayer()
            && self.link.is_none() && !self.runahead_unavailable
            && self.core.speculative_audio_supported()
    }

    /// Reset after each telemetry interval without touching persistent game state.
    pub(crate) fn take_runahead_probe(&mut self) -> RunaheadProbe {
        std::mem::take(&mut self.runahead_probe)
    }

    /// Roll back both standard libretro state and core-specific transient
    /// audio: gpSP's sound ring, or mGBA's converter/filter/queue sidecar.
    pub fn prepare_runahead(&mut self, buttons: u16, turbo: bool, frames: u8) {
        let started = Instant::now();
        self.runahead_video.clear();
        if frames == 0 || frames > 2 || self.runahead_rollback_failed || self.mgba_flash_runahead_guard ||
            self.sma4_runahead_guard ||
            self.runahead_unavailable || self.in_multiplayer() { return; }
        if self.prepare_runahead_second_instance(buttons, turbo, frames) { return; }
        if !self.core.speculative_audio_supported() {
            self.runahead_probe.failures += 1;
            self.runahead_unavailable = true;
            log_launch_timing("Run-Ahead disabled: core has no safe speculative audio support");
            return;
        }
        if let Err(error) = self.core.serialize_into(&mut self.runahead_state) {
            self.runahead_probe.failures += 1;
            self.runahead_unavailable = true;
            log_launch_timing(&format!("Run-Ahead disabled: serialize failed: {error}"));
            return;
        }
        if let Err(error) = self.core.capture_speculative_audio() {
            self.runahead_probe.failures += 1;
            self.runahead_unavailable = true;
            log_launch_timing(&format!("Run-Ahead disabled: audio snapshot failed: {error}"));
            return;
        }
        let snapshot_us = started.elapsed().as_micros() as u64;
        let predicted = Instant::now();
        let pressed = ButtonMask(buttons);
        // Turbo flips every three REAL frames. Each predicted frame must
        // sample its own future phase; reusing one mask for all offsets
        // predicts incorrect key presses across a three-frame boundary.
        for future in 0..frames {
            let mapped = xy_buttons(pressed, turbo,
                self.frame_count.wrapping_add(u32::from(future)));
            self.core.run_speculative_frame(mapped);
            // Discard predicted audio without reallocating libretro's buffer.
            self.core.discard_audio();
        }
        self.runahead_video.extend_from_slice(self.core.video_xrgb8888());
        let speculative_us = predicted.elapsed().as_micros() as u64;
        let restore_started = Instant::now();
        let restored = self.core.restore_speculative_state(&self.runahead_state)
            .and_then(|_| self.core.restore_speculative_audio());
        let restore_us = restore_started.elapsed().as_micros() as u64;
        self.runahead_probe.calls = self.runahead_probe.calls.saturating_add(1);
        self.runahead_probe.snapshots_us += snapshot_us;
        self.runahead_probe.speculative_us += speculative_us;
        self.runahead_probe.restores_us += restore_us;
        self.runahead_probe.max_us = self.runahead_probe.max_us.max(
            started.elapsed().as_micros() as u64);
        if let Err(error) = restored {
            self.runahead_probe.failures += 1;
            // A speculative frame HAS mutated the core. Disabling Run-Ahead
            // alone would continue gameplay and silently overwrite a valid
            // save with the wrong timeline. Fail CLOSED: freeze the session,
            // reject every automatic/export/manual/periodic save and only
            // allow a separately confirmed discard without writing.
            self.runahead_video.clear();
            self.runahead_unavailable = true;
            self.runahead_rollback_failed = true;
            log_launch_timing(&format!(
                "Run-Ahead ROLLBACK FAILED: session quarantined, ALL saves disabled: {error}"));
        }

    }

    /// RetroArch "second instance" Run-Ahead. Returns false when the copy is
    /// unavailable, so the caller uses the single-instance path instead.
    /// The real core is only serialized, never rewound: a failure here can
    /// never corrupt the real timeline.
    fn prepare_runahead_second_instance(&mut self, buttons: u16, turbo: bool, frames: u8) -> bool {
        if self.runahead_secondary_unavailable { return false; }
        let started = Instant::now();
        if self.runahead_secondary.is_none() {
            let Some(spec) = self.runahead_secondary_spec.as_ref() else {
                self.runahead_secondary_unavailable = true;
                return false;
            };
            let battery = self.core.save_ram_verified().ok().flatten();
            match crate::runahead_secondary::open(spec, battery.as_deref()) {
                Ok(copy) => {
                    log_launch_timing(&format!(
                        "Run-Ahead second instance ready in {}ms", started.elapsed().as_millis()));
                    self.runahead_secondary = Some(copy);
                    if self.solar.is_some() {
                        let luminance = self.sun_luminance.unwrap_or_else(||
                            crate::emulation::lux_luminance(slot_retro::sensors::illuminance()));
                        if let Some(copy) = self.runahead_secondary.as_mut() { copy.set_sun(luminance); }
                    }
                }
                Err(error) => {
                    self.runahead_secondary_unavailable = true;
                    log_launch_timing(&format!(
                        "Run-Ahead second instance unavailable, using single instance: {error}"));
                    return false;
                }
            }
        }
        if let Err(error) = self.core.serialize_into(&mut self.runahead_state) {
            self.runahead_probe.failures += 1;
            self.runahead_unavailable = true;
            log_launch_timing(&format!("Run-Ahead disabled: serialize failed: {error}"));
            return true;
        }
        let snapshot_us = started.elapsed().as_micros() as u64;
        let predicted = Instant::now();
        let pressed = ButtonMask(buttons);
        // Turbo flips every three REAL frames; each predicted frame samples
        // its own future phase, exactly like the single-instance path.
        let mut inputs = [ButtonMask::default(); 2];
        for future in 0..usize::from(frames) {
            inputs[future] = xy_buttons(pressed, turbo,
                self.frame_count.wrapping_add(future as u32));
        }
        let copy = self.runahead_secondary.as_deref_mut().expect("booted above");
        match crate::runahead_secondary::predict(copy, &self.runahead_state,
            &inputs[..usize::from(frames)]) {
            Ok(video) => self.runahead_video.extend_from_slice(video),
            Err(error) => {
                // Only the copy is affected: drop it and keep playing.
                self.runahead_probe.failures += 1;
                self.runahead_secondary = None;
                self.runahead_secondary_unavailable = true;
                log_launch_timing(&format!(
                    "Run-Ahead second instance stopped, using single instance: {error}"));
                return false;
            }
        }
        let speculative_us = predicted.elapsed().as_micros() as u64;
        self.runahead_probe.calls = self.runahead_probe.calls.saturating_add(1);
        self.runahead_probe.second_instance_calls =
            self.runahead_probe.second_instance_calls.saturating_add(1);
        self.runahead_probe.snapshots_us += snapshot_us;
        self.runahead_probe.speculative_us += speculative_us;
        self.runahead_probe.max_us = self.runahead_probe.max_us.max(
            started.elapsed().as_micros() as u64);
        true
    }

    /// RetroArch "Preemptive Frames" instead of Run-Ahead: `frames` real
    /// frames are kept and replayed when the buttons change. 0 turns it off.
    /// The Run-Ahead guards apply: Pokémon FLASH1M, SMA4, Link, and cores
    /// whose audio cannot be rolled back.
    pub fn set_preemptive_frames(&mut self, frames: u8) {
        let allowed = !self.runahead_rollback_failed && !self.runahead_unavailable
            && !self.mgba_flash_runahead_guard && !self.sma4_runahead_guard
            && !self.in_multiplayer()
            && self.core.speculative_audio_supported();
        self.preempt.set_frames(if allowed { usize::from(frames) } else { 0 });
    }

    /// Before each real solo frame: replay the window with the new buttons
    /// if they changed, then keep the state this frame starts from. False
    /// only when a rollback failed and the session was quarantined.
    fn preempt_before_frame(&mut self, buttons: u16, turbo: bool) -> bool {
        let n = self.preempt.frames();
        if n == 0 || self.in_multiplayer() { return true; }
        let input = (buttons, turbo);
        if self.preempt.needs_rollback(input) {
            let started = Instant::now();
            // The replayed frames rewrite the last n frames with the new
            // buttons; their audio was already played and is dropped. The
            // Run-Ahead audio sidecar is NOT restored afterwards: it holds
            // the old timeline's sound channels, and restoring it would
            // silence a sound the new buttons just started (a GB jump sound
            // heard frames late). After the replay the core's audio already
            // belongs to the new timeline, as in RetroArch.
            let pressed = ButtonMask(buttons);
            let first = self.frame_count.wrapping_sub(n as u32);
            let mut replayed = self.core.restore_speculative_state(self.preempt.state(0));
            for k in 0..n {
                if replayed.is_err() { break; }
                if k > 0 { replayed = self.core.serialize_into(self.preempt.state_mut(k)); }
                if replayed.is_err() { break; }
                self.core.run_speculative_frame(
                    xy_buttons(pressed, turbo, first.wrapping_add(k as u32)));
                self.core.discard_audio();
            }
            let elapsed_us = started.elapsed().as_micros() as u64;
            self.runahead_probe.preemptive_rollbacks =
                self.runahead_probe.preemptive_rollbacks.saturating_add(1);
            self.runahead_probe.restores_us += elapsed_us;
            self.runahead_probe.max_us = self.runahead_probe.max_us.max(elapsed_us);
            if let Err(error) = replayed {
                // The core is somewhere inside the window: fail closed
                // exactly like a failed Run-Ahead rollback.
                self.runahead_probe.failures += 1;
                self.runahead_unavailable = true;
                self.runahead_rollback_failed = true;
                self.preempt.set_frames(0);
                log_launch_timing(&format!(
                    "Preemptive Frames ROLLBACK FAILED: session quarantined, ALL saves disabled: {error}"));
                return false;
            }
        }
        let started = Instant::now();
        if let Err(error) = self.core.serialize_into(self.preempt.push(input)) {
            self.runahead_probe.failures += 1;
            self.runahead_unavailable = true;
            self.preempt.set_frames(0);
            log_launch_timing(&format!("Preemptive Frames disabled: serialize failed: {error}"));
            return true;
        }
        self.runahead_probe.calls = self.runahead_probe.calls.saturating_add(1);
        self.runahead_probe.snapshots_us += started.elapsed().as_micros() as u64;
        true
    }

    pub fn rewind_step(&mut self) -> bool {
        if self.runahead_rollback_failed || self.in_multiplayer() { return false; }
        self.runahead_video.clear();
        self.preempt.clear();
        // Rewind is a real state restoration: finish any older disk snapshot
        // once before moving the core backward in time.
        self.flush_periodic_battery();
        let Some(previous) = self.rewind.pop() else { return false; };
        if let Err(error) = self.core.unserialize(&previous) {
            // A failed state restore must never silently eject the cartridge.
            // Leave the game alive and surface a diagnostic for real-device QA.
            log_launch_timing(&format!(
                "Rewind state restore was refused; staying in game: {error}"));
            return false;
        }
        // Match original Slot's rewind: force frame rendering (never inherit
        // an FF/speculative frame-skip state) before refreshing the picture.
        self.core.set_frame_skip(false);
        self.core.run_frame(ButtonMask(0));
        let _ = self.core.take_audio();
        self.rewind_frames = 0;
        true
    }

    pub fn can_change_gb_palette(&self) -> bool {
        self.palette_live
    }

    pub fn set_gb_palette(&mut self, name: &str) {
        if self.palette_live && self.core_kind == Core::Gambatte {
            self.set_gb_palette_mode(true, name);
        } else if self.palette_live {
            self.set_core_option("mgba_gb_colors_preset", "0");
            self.set_core_option("mgba_gb_colors", name);
        }
    }

    /// Live option change on the real core, mirrored to the second-instance
    /// prediction copy so predicted pictures keep the same colours.
    fn set_core_option(&mut self, key: &str, value: &str) {
        self.core.set_option(key, value);
        if let Some(spec) = self.runahead_secondary_spec.as_mut() { spec.set_option(key, value); }
        if let Some(copy) = self.runahead_secondary.as_mut() { copy.set_option(key, value); }
    }

    /// Per-game GB palette enable/disable is applied through the core's live
    /// options, without restarting libretro or touching ROM SRAM/RTC.
    pub fn set_gb_palette_mode(&mut self, enabled: bool, name: &str) {
        if !self.palette_live { return; }
        if self.core_kind == Core::Gambatte {
            match crate::gb_core_switch::gambatte_palette(name).filter(|_| enabled) {
                Some(palette) => {
                    self.set_core_option("gambatte_gb_colorization", "internal");
                    self.set_core_option("gambatte_gb_internal_palette", palette);
                }
                None => self.set_core_option("gambatte_gb_colorization", "GBC"),
            }
            return;
        }
        self.set_core_option("mgba_gb_colors_preset", if enabled { "0" } else { "1" });
        if enabled { self.set_core_option("mgba_gb_colors", name); }
    }

    pub fn set_colour_correction(&mut self, enabled: bool) {
        let (key, value) = match self.core_kind {
            Core::Mgba => ("mgba_color_correction",
                if enabled { "Auto" } else { "OFF" }),
            Core::Gpsp => ("gpsp_color_correction",
                if enabled { "enabled" } else { "disabled" }),
            Core::Gambatte => ("gambatte_gbc_color_correction",
                if enabled { "GBC only" } else { "disabled" }),
        };
        self.set_core_option(key, value);
    }

    pub fn solar(&self) -> bool { self.solar.is_some() }

    /// The highest manual sun of this solar game, one per gauge bar.
    pub fn sun_top(&self) -> u8 {
        crate::emulation::sun_top(self.solar == Some(Solar::Boktai1))
    }

    /// Boktai's sun, live: mGBA reads it the next time the game samples
    /// its solar sensor. SENSOR follows the light sensor every frame.
    pub fn set_sun(&mut self, sun: u8) {
        let Some(solar) = self.solar else { return };
        let luminance = crate::emulation::sun_luminance(sun, solar == Solar::Boktai1);
        slot_retro::sensors::set_light_wanted(luminance.is_none());
        self.sun_luminance = luminance;
        self.give_sun(luminance.unwrap_or_else(||
            crate::emulation::lux_luminance(slot_retro::sensors::illuminance())));
    }

    fn give_sun(&mut self, luminance: u8) {
        self.core.set_sun(luminance);
        if let Some(copy) = self.runahead_secondary.as_mut() { copy.set_sun(luminance); }
    }

    fn follow_light_sensor(&mut self) {
        if self.solar.is_some() && self.sun_luminance.is_none() {
            self.give_sun(crate::emulation::lux_luminance(slot_retro::sensors::illuminance()));
        }
    }

    pub fn rumble_strength(&self) -> u16 {
        self.core.rumble().strength()
    }

    pub fn rewind_fill(&self) -> u8 {
        self.rewind.fill()
    }

    pub fn frame(&self) -> &[u8] {
        if !self.runahead_video.is_empty() { &self.runahead_video }
        else { self.core.video_xrgb8888() }
    }

    pub fn take_audio(&mut self) -> Vec<i16> {
        self.core.take_audio()
    }

    /// Caller consumed the REAL frame's PCM (or muted fast-forward PCM).
    /// Return its allocation to the libretro host before the next frame.
    /// Audio stays owned by the core during speculative Run-Ahead.
    pub fn recycle_audio(&mut self, audio: Vec<i16>) {
        self.core.recycle_audio(audio);
    }

    /// Build immutable real-core data on GL, but never do the 30-second
    /// fsync/rename on a frame callback. No speculative data crosses to disk.
    fn periodic_battery_snapshot(&self) -> Result<BatterySnapshot, String> {
        if self.runahead_rollback_failed {
            return Err("Unsafe Run-Ahead timeline; periodic save blocked".into());
        }
        let ram = self.core.save_ram_verified()
            .map_err(|error| format!("Active battery snapshot: {error}"))?;
        if ram.as_ref().is_some_and(|bytes| self.pokemon_flash_would_regress(bytes)) {
            return Err("Pokémon Flash save incomplete; prior checkpoint protected".into());
        }
        Ok(BatterySnapshot {
            save: self.save.clone(), rtc_path: self.rtc.clone(),
            ram, rtc: self.core.save_rtc(),
        })
    }

    /// Order all earlier optional writes before any newer mandatory write.
    /// Errors are logged; a fresh checked checkpoint below must still be
    /// attempted, because it can repair an earlier storage failure.
    fn flush_periodic_battery(&mut self) {
        if let Some(writer) = self.battery_writer.as_mut() {
            if let Err(error) = writer.flush() {
                log_launch_timing(&format!(
                    "F7 periodic checkpoint failed before mandatory save: {error}"));
            }
        }
    }

    fn save_sram(&mut self) -> bool {
        if let Some(writer) = self.battery_writer.as_mut() {
            // Poll only every ~30 seconds; one pending snapshot maximum.
            // When disk is slow, skip this OPTIONAL generation, never wait
            // for an fsync on the GL callback or enqueue stale copies.
            if let Err(error) = writer.poll() {
                log_launch_timing(&format!("F7 periodic battery write failed: {error}"));
            }
            if writer.busy() {
                // Storage is still busy: throttle the next non-blocking poll
                // rather than querying the channel on every emulated frame.
                self.last_sram = Instant::now();
                return false;
            }
            let snapshot = match self.periodic_battery_snapshot() {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    self.last_sram = Instant::now();
                    log_launch_timing(&format!("F7 battery snapshot rejected: {error}"));
                    return false;
                }
            };
            let has_data = snapshot.ram.is_some() || snapshot.rtc.is_some();
            if !has_data {
                self.last_sram = Instant::now();
                return false;
            }
            let outcome = self.battery_writer.as_mut()
                .expect("writer previously verified").enqueue(snapshot);
            match outcome {
                Ok(()) => {
                    self.last_sram = Instant::now();
                    true
                }
                Err(error) => {
                    // A failed worker must not silently disable 30-second
                    // checkpoints for the rest of the session. Join it first
                    // (no stale concurrent write), then use the proven sync
                    // implementation until this game is closed.
                    log_launch_timing(&format!(
                        "F7 battery writer unavailable, falling back to sync: {error}"));
                    self.battery_writer.take();
                    match self.save_sram_checked() {
                        Ok(written) => written,
                        Err(error) => {
                            log_launch_timing(&format!(
                                "Battery/RTC fallback write failed: {error}"));
                            false
                        }
                    }
                }
            }
        } else {
            // No worker can be started, or the core is a private Link sandbox.
            match self.save_sram_checked() {
                Ok(written) => written,
                Err(error) => {
                    log_launch_timing(&format!("Battery/RTC periodic write failed: {error}"));
                    false
                }
            }
        }
    }

    /// A Pokémon Gen III image with no complete bank only threatens a save
    /// when the file on disk holds a complete one. A new game that was never
    /// saved in-game (erased 0xFF Flash) has nothing to protect: refusing it
    /// made every Save & Eject fail with "Couldn't save".
    fn pokemon_flash_would_regress(&self, bytes: &[u8]) -> bool {
        self.mgba_flash_runahead_guard && pokemon_flash_complete_bank_count(bytes) == 0
            && std::fs::read(&self.save)
                .is_ok_and(|disk| pokemon_flash_complete_bank_count(&disk) > 0)
    }

    /// The battery and RTC are independent persistent files. The legacy
    /// boolean used OR, so a successful RTC could hide a failed battery write.
    /// Return every attempted file write's error to callers that must retain
    /// the session (Save & Eject); none means the cartridge has no such file.
    /// The state in the layout of the RetroArch core that will load it.
    fn for_retroarch(&self, state: Vec<u8>) -> Vec<u8> {
        if !self.gambatte_legacy_export { return state; }
        crate::gambatte_state::strip_sachen(&state).unwrap_or(state)
    }

    fn save_sram_checked(&mut self) -> Result<bool, String> {
        // Mandatory suspend/eject: wait for all older periodic generations
        // before writing fresh SRAM/RTC and optionally a matching state.
        self.flush_periodic_battery();
        if self.runahead_rollback_failed {
            return Err("Run-Ahead rollback failed; unsafe timeline cannot be saved".into());
        }
        let mut wrote = false;
        let mut error: Option<String> = None;
        match self.core.save_ram_verified()
            .map_err(|e| format!("Active battery snapshot: {e}"))? {
            Some(bytes) if self.pokemon_flash_would_regress(&bytes) => {
                error = Some("Pokémon Flash save is incomplete; previous save protected".into());
            }
            Some(bytes) => match atomic_write(&self.save, &bytes) {
                Ok(()) => wrote = true,
                Err(err) => error = Some(format!("Battery save: {err}")),
            },
            None => {}
        }
        if let Some(bytes) = self.core.save_rtc() {
            match atomic_write(&self.rtc, &bytes) {
                Ok(()) => wrote = true,
                Err(err) => {
                    let reason = format!("RTC save: {err}");
                    if let Some(previous) = error.as_mut() {
                        previous.push_str("; ");
                        previous.push_str(&reason);
                    } else { error = Some(reason); }
                }
            }
        }
        self.last_sram = Instant::now();
        error.map_or(Ok(wrote), Err)
    }

    /// Mandatory ejection checkpoint: do not drop the only live core if its
    /// primary battery/RTC/state/export cannot be persisted. This never
    /// rewrites a corrupt battery over a known-good file. An absent battery
    /// on a ROM that does not implement SRAM is not an I/O failure.
    pub fn save_for_eject(&mut self, with_state: bool) -> Result<bool, String> {
        // A failed Link session is no longer a valid solo timeline.  The
        // original SOLO checkpoint was preserved *before* creating its
        // private dual-core sandbox. Never ask the stalled native Link core
        // to capture/save SRAM, RTC or a state during "Eject to play solo":
        // its incomplete frame is intentionally uncommittable. Ejection
        // simply discards the unsafe sandbox; reinsertion restores SOLO.
        if self.requires_relaunch() {
            log_launch_timing(
                "Eject after failed Link: discarding private sandbox without native save"
            );
            return Ok(false);
        }
        if self.runahead_rollback_failed {
            return Err("Run-Ahead rollback failed; exit without saving instead".into());
        }
        let battery = self.save_sram_checked()?;
        if !with_state || self.in_multiplayer() {
            return Ok(battery);
        }
        let data = self.core.serialize()
            .map_err(|e| format!("Auto state capture: {e}"))?;
        atomic_write(&self.state, &data)
            .map_err(|e| format!("Auto state write: {e}"))?;
        let container = retroarch_state::encode(&self.for_retroarch(data))
            .map_err(|e| format!("RetroArch state encoding: {e}"))?;
        atomic_write(&self.retroarch_export, &container)
            .map_err(|e| format!("RetroArch state export: {e}"))?;
        Ok(true)
    }

    pub fn save(&mut self, with_state: bool) -> bool {
        // Suspend/Android lifecycle also requires a fully durable generation
        // before requesting RetroArch SAF export. It must not treat an RTC
        // success as proof that SRAM/state writes succeeded.
        match self.save_for_eject(with_state) {
            Ok(saved) => saved,
            Err(error) => {
                log_launch_timing(&format!(
                    "Save failed; keeping existing on-disk checkpoint: {error}"));
                false
            }
        }
    }
    /// Original Slot ring: 10 timestamped states per ROM/core, each with
    /// a 240×160 PNG thumbnail. Independent from RetroArch's manual slots.
    pub fn save_manual(&mut self) -> Result<String, String> {
        if self.runahead_rollback_failed { return Err("Unsafe Run-Ahead state; manual save blocked".into()); }
        if self.in_multiplayer() { return Err("Save States unavailable for Multiplayer sessions".into()); }
        let state = self.core.serialize().map_err(|e|e.to_string())?;
        let preview = thumb::png(self.core.video_xrgb8888()).unwrap_or_default();
        let entries = self.ring.list().map_err(|e|e.to_string())?;
        let mut seconds = slot_store::parse_stamp(&stamp_now()).ok_or("Invalid system clock")?;
        let mut stamp = format_stamp(seconds);
        while entries.iter().any(|e|e.stamp==stamp) {
            seconds+=1;
            stamp=format_stamp(seconds);
        }
        let evicted = if entries.len()>=RING_MAX {
            entries.last().and_then(|item|
                self.ring.read(&item.stamp).ok()
                    .map(|(state, thumb)|(item.stamp.clone(),state,thumb)))
        } else {None};
        self.ring.push(&state, &preview, &stamp).map_err(|e|e.to_string())?;
        self.pending_undo = Some((Instant::now(), PendingUndo::Saved {
            stamp: stamp.clone(), evicted
        }));
        // Keep Slot's Polaroid history private. The Android storage worker
        // publishes a distinct, verified RetroArch .stateN after this returns.
        let encoded = retroarch_state::encode(&self.for_retroarch(state.clone()))?;
        let export = std::path::PathBuf::from(format!(
            "{}.{}.rastate", self.manual_export_prefix.display(), stamp
        ));
        atomic_write(&export, &encoded).map_err(|error|
            format!("Polaroid saved, manual RetroArch export failed: {error}"))?;
        Ok(stamp)
    }

    pub fn history(&self) -> Vec<StateEntry> {
        self.ring.list().unwrap_or_default()
    }

    pub fn load_manual(&mut self, stamp:&str) -> Result<(), String> {
        if self.runahead_rollback_failed { return Err("Run-Ahead state unsafe; restart from disk".into()); }
        if self.in_multiplayer() { return Err("Load State unavailable during Multiplayer".into()); }
        let (bytes, _) = self.ring.read(stamp).map_err(|e|e.to_string())?;
        let prior = self.core.serialize().map_err(|e|e.to_string())?;
        // Ensure a queued newer-generation battery write cannot race with
        // this explicit time-travel restore (the game state may move back).
        self.flush_periodic_battery();
        self.preempt.clear();
        self.core.unserialize(&bytes).map_err(|e|e.to_string())?;
        self.pending_undo=Some((Instant::now(), PendingUndo::Loaded {prior}));
        Ok(())
    }

    pub fn load_latest(&mut self)->Result<(),String> {
        let newest=self.history().into_iter().next()
            .ok_or_else(||"No saved states yet".to_owned())?;
        self.load_manual(&newest.stamp)
    }

    pub fn delete_manual(&mut self, stamp:&str)->Result<(),String> {
        if self.in_multiplayer() { return Err("Save States unavailable during Multiplayer".into()); }
        self.ring.remove(stamp).map_err(|e|e.to_string())
    }

    pub fn undo_manual(&mut self)->Result<(),String> {
        if self.runahead_rollback_failed { return Err("Run-Ahead state unsafe; restart from disk".into()); }
        if self.in_multiplayer() { return Err("Undo unavailable during Multiplayer".into()); }
        let Some((since,_))=&self.pending_undo else { return Err("Nothing to undo".into())};
        if since.elapsed()>UNDO_WINDOW {
            self.pending_undo=None;
            return Err("Undo expired (30 seconds)".into());
        }
        // An undo may restore an earlier memory timeline.
        self.flush_periodic_battery();
        let (_, pending)=self.pending_undo.take().unwrap();
        self.preempt.clear();
        match pending {
            PendingUndo::Loaded {prior} =>
                self.core.unserialize(&prior).map_err(|e|e.to_string()),
            PendingUndo::Saved {stamp,evicted} => {
                self.ring.remove(&stamp).map_err(|e|e.to_string())?;
                if let Some((name,state,thumb))=evicted {
                    self.ring.push(&state,&thumb,&name).map_err(|e|e.to_string())?;
                }
                Ok(())
            }
        }
    }

    pub fn undo_available(&self)->bool {
        !self.in_multiplayer() && self.pending_undo.as_ref().is_some_and(|(at,_)|at.elapsed()<=UNDO_WINDOW)
    }

}

impl Drop for GameSession {
    fn drop(&mut self) {
        // Ejection, Android lifecycle teardown and failed launches must tell
        // the remote console the link ended, not leave an orphaned TCP session.
        self.end_multiplayer();
    }
}

#[cfg(test)]
mod pokemon_flash_guard_tests {
    use super::*;
    #[test]
    fn game_boy_audio_keeps_its_131072hz_rate() {
        assert_eq!(core_sample_rate(131_072), 131_072, "mGBA GB/GBC");
        assert_eq!(core_sample_rate(65_536), 65_536, "mGBA GBA, gpSP");
        assert_eq!(core_sample_rate(1_000_000), 192_000);
        assert_eq!(core_sample_rate(0), 8_000);
    }
    #[test]
    fn flash_guard_accepts_complete_checksum_banks_and_rejects_partial_erase() {
        fn write_sector(image: &mut [u8], physical: usize, section: usize, counter: u32) {
            let at = physical * 4096;
            let s = &mut image[at..at + 4096];
            s.fill(0);
            s[0] = section as u8;
            s[0xFF4..0xFF6].copy_from_slice(&(section as u16).to_le_bytes());
            s[0xFF8..0xFFC].copy_from_slice(&0x0801_2025u32.to_le_bytes());
            s[0xFFC..0x1000].copy_from_slice(&counter.to_le_bytes());
            let checksum = section as u16;
            s[0xFF6..0xFF8].copy_from_slice(&checksum.to_le_bytes());
        }
        let mut image = vec![0xFF; 128 * 1024];
        assert_eq!(pokemon_flash_complete_bank_count(&image), 0);
        for i in 0..14 {
            write_sector(&mut image, i, i, 22);
            write_sector(&mut image, 14+i, i, 21);
        }
        assert_eq!(pokemon_flash_complete_bank_count(&image), 2);
        image[0] ^= 0x01;
        assert_eq!(pokemon_flash_complete_bank_count(&image), 1);
        image[14 * 4096 + 0xFF8] = 0xff;
        assert_eq!(pokemon_flash_complete_bank_count(&image), 0);
        image.truncate(65536);
        assert_eq!(pokemon_flash_complete_bank_count(&image), 0);
    }

    #[test]
    fn drill_dozer_disables_only_forced_mgba_gbp_rumble() {
        let dir = std::env::temp_dir().join(format!("slot-drill-dozer-gbp-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("game-renamed-unrelated.gba");
        for code in [*b"V49E", *b"V49J", *b"V49P"] {
            let mut rom = vec![0u8; 192];
            rom[0xAC..0xB0].copy_from_slice(&code);
            std::fs::write(&path, rom).unwrap();
            assert!(drill_dozer_mgba_gbp_bypass(&path, Core::Mgba, Platform::Gba));
            assert!(!drill_dozer_mgba_gbp_bypass(&path, Core::Gpsp, Platform::Gba));
            assert!(!drill_dozer_mgba_gbp_bypass(&path, Core::Mgba, Platform::Gb));
        }
        for code in [*b"AX4E", *b"BPGE", *b"V48E", *b"V49-"] {
            let mut rom = vec![0u8; 192];
            rom[0xAC..0xB0].copy_from_slice(&code);
            std::fs::write(&path, rom).unwrap();
            assert!(!drill_dozer_mgba_gbp_bypass(&path, Core::Mgba, Platform::Gba));
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mario_advance_four_blocks_speculative_frames_for_both_gba_cores() {
        let dir = std::env::temp_dir().join(format!("slot-sma4-guard-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("patched-e-reader-rom.gba");
        for code in [*b"AX4E",*b"AX4P",*b"AX4J"] {
            let mut rom = vec![0u8; 192];
            rom[0xAC..0xB0].copy_from_slice(&code);
            std::fs::write(&path, &rom).unwrap();
            assert!(sma4_runahead_guard(&path, Platform::Gba));
            assert!(!sma4_runahead_guard(&path, Platform::Gb));
        }
        for code in [*b"AMAE",*b"AXVE",*b"AXPE",*b"    "] {
            let mut rom = vec![0u8; 192];
            rom[0xAC..0xB0].copy_from_slice(&code);
            std::fs::write(&path, &rom).unwrap();
            assert!(!sma4_runahead_guard(&path, Platform::Gba));
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn per_rom_guard_is_mgba_pokemon_gen3_only() {
        for code in [*b"BPRE",*b"BPGE",*b"BPGJ",*b"BPEE",*b"AXVE",*b"AXPJ"] {
            assert!(pokemon_gen3_flash_code(&code), "{code:?}");
        }
        for code in [*b"AFZE",*b"AWRE",*b"BRBE",*b"    "] {
            assert!(!pokemon_gen3_flash_code(&code), "{code:?}");
        }
        let dir = std::env::temp_dir().join(format!("slot-flash-guard-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("renamed-localized-game.gba");
        let mut rom = vec![0u8; 192];
        rom[0xAC..0xB0].copy_from_slice(b"BPGE");
        std::fs::write(&path, &rom).unwrap();
        assert!(mgba_flash_runahead_guard(&path, Core::Mgba, Platform::Gba));
        assert!(!mgba_flash_runahead_guard(&path, Core::Gpsp, Platform::Gba));
        assert!(!mgba_flash_runahead_guard(&path, Core::Mgba, Platform::Gb));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod a13r3_generation_rejoin_tests {
    use super::*;
    use std::fs;

    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("slot-a13r3-rejoin-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn protected_retroarch_export_checkpoint_uses_active_generation() {
        let root = fixture();
        let id = "a371cbb437f277426a3303a7e7cd83e6f38f0f426ff3b1f827ecc11fa3e25742";
        let plain = format!("{id}.mgba.retroarch-export.state.auto");
        assert_eq!(active_export_filename(&root, id, Core::Mgba).unwrap(), plain);
        let key = format!("{id}.mgba.gen-1792007598729624196");
        fs::write(root.join(format!("{id}.mgba.state-generation.active")), &key).unwrap();
        let name = active_export_filename(&root, id, Core::Mgba).unwrap();
        assert_eq!(name, format!("{key}.retroarch-export.state.auto"));
        assert!(export_path_matches_snapshot(&root.join(&name), &name));
        assert!(!export_path_matches_snapshot(&root.join(&plain), &name));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generation_switch_during_link_remains_fail_closed() {
        let root = fixture();
        let id = "abe34";
        let record = root.join(format!("{id}.mgba.state-generation.active"));
        let first = format!("{id}.mgba.gen-11111");
        fs::write(&record, &first).unwrap();
        let expected = active_export_filename(&root, id, Core::Mgba).unwrap();
        fs::write(&record, format!("{id}.mgba.gen-22222")).unwrap();
        let actual = active_export_filename(&root, id, Core::Mgba).unwrap();
        assert!(!export_path_matches_snapshot(&root.join(&actual), &expected));
        fs::write(&record, "../other").unwrap();
        assert!(active_export_filename(&root, id, Core::Mgba).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod solo_recovery_tests {
    use super::*;
    use slot_retro::{AvInfo, CoreError};

    struct CheckpointCore {
        state: Vec<u8>,
        reject_restore: bool,
        corrupt_verification: bool,
    }
    impl RetroCore for CheckpointCore {
        fn load(&mut self, _: &Path) -> Result<(), CoreError> { Ok(()) }
        fn run_frame(&mut self, _: ButtonMask) {}
        fn video_xrgb8888(&self) -> &[u8] { &[] }
        fn take_audio(&mut self) -> Vec<i16> { vec![] }
        fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
            if self.corrupt_verification { Ok(vec![0; 8]) }
            else { Ok(self.state.clone()) }
        }
        fn unserialize(&mut self, data: &[u8]) -> Result<(), CoreError> {
            if self.reject_restore {
                return Err(CoreError::State("wrong core state".into()));
            }
            self.state = data.to_vec();
            Ok(())
        }
        fn save_ram(&self) -> Option<Vec<u8>> { None }
        fn load_save_ram(&mut self, _: &[u8]) -> Result<(), CoreError> { Ok(()) }
        fn av_info(&self) -> AvInfo {
            AvInfo { fps: 59.7275, sample_rate: 32768.0 }
        }
    }

    #[test]
    fn failed_link_reopens_exact_solo_checkpoint_never_empty_title_state() {
        let mut core = CheckpointCore {
            state: vec![0xA1; 0x800], reject_restore: false,
            corrupt_verification: false,
        };
        let original = vec![0xB2; 0x800];
        restore_solo_checkpoint_to_core(&mut core, &original).unwrap();
        assert_eq!(core.state, original);
        let mut paired = original.clone();
        paired[..4].copy_from_slice(b"SLK1");
        assert!(restore_solo_checkpoint_to_core(&mut core, &paired).is_err());
        assert_eq!(core.state, original);
    }

    #[test]
    fn rejected_or_invalidly_serialized_checkpoint_must_report_failure() {
        let saved = vec![0xBA; 0x800];
        let mut core = CheckpointCore {
            state: vec![0x11; 0x800], reject_restore: true,
            corrupt_verification: false,
        };
        assert!(restore_solo_checkpoint_to_core(&mut core, &saved).is_err());
        assert_eq!(core.state, vec![0x11; 0x800]);
        core.reject_restore = false;
        core.corrupt_verification = true;
        assert!(restore_solo_checkpoint_to_core(&mut core, &saved).is_err(),
            "successful unserialize without valid serialization is not recovery");
    }
}

#[cfg(test)]
mod xy_mapping_tests {
    use super::*;
    #[test]
    fn ended_link_never_falls_through_to_solo_frames_after_android_sleep() {
        assert!(emulation_may_advance(false, false), "normal solo works");
        assert!(emulation_may_advance(true, true), "active Cable/RFU link works");
        assert!(!emulation_may_advance(true, false),
            "End Link, Wi-Fi loss or onPause must freeze the linked core");
    }

    #[test]
    fn classic_turbo_keeps_the_original_slot_x_y_behavior() {
        for frame in 0..12 {
            let buttons = ButtonMask(ButtonMask::X | ButtonMask::Y);
            assert_eq!(xy_buttons(buttons, true, frame), buttons.turbo(frame));
        }
    }
    #[test]
    fn ab_mode_maps_y_to_a_and_x_to_b_without_losing_original_buttons() {
        let y = xy_buttons(ButtonMask(ButtonMask::Y), false, 0);
        let x = xy_buttons(ButtonMask(ButtonMask::X), false, 3);
        assert_eq!(y.0, ButtonMask::A);
        assert_eq!(x.0, ButtonMask::B);
        let both = xy_buttons(ButtonMask(ButtonMask::A | ButtonMask::B |
            ButtonMask::X | ButtonMask::Y | ButtonMask::RIGHT), false, 5);
        assert_eq!(both.0, ButtonMask::A | ButtonMask::B | ButtonMask::RIGHT);
    }
}
