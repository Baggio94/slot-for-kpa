//! Native libretro mGBA / gpSP game session. All core calls occur on GLSurfaceView's GL thread.
//! Android's SAF bridge materializes a **bounded private cache** copy of each selected ROM.
use crate::pokemon_flash::pokemon_flash_complete_bank_count;
use slot_retro::{ButtonMask, LibretroCore, RetroCore, LinkChannel};
use crate::link_kind::LinkKind;
use crate::link_play::{Frame as LinkFrame, LinkError, LinkPlay};
use crate::multiplayer::Role;
use slot_store::{atomic_write, Core, Platform, StateEntry, StateRing, stamp_now, parse_stamp, format_stamp, RING_MAX};
use crate::thumb;
use crate::retroarch_state;
use crate::rewind::{RewindThread, REWIND_BYTES};
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

/// Cartridge header code, not a localized filename: FireRed, LeafGreen,
/// Emerald, Ruby and Sapphire, across original game-code regions.
fn pokemon_gen3_flash_code(code: &[u8; 4]) -> bool {
    [b"BPR", b"BPG", b"BPE", b"AXV", b"AXP"]
        .iter().any(|prefix| code.starts_with(*prefix))
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
    retroarch_import: PathBuf,
    ring: StateRing,
    rewind: RewindThread,
    rewind_frames: u8,
    frame_count: u32,
    // Presentation-only pixels from speculative frames; save/RA keep the real state.
    runahead_video: Vec<u8>,
    // Preserve the libretro state allocation across speculative frames.
    runahead_state: Vec<u8>,
    runahead_unavailable: bool,
    // Keep mGBA speculative frames away from Pokémon Gen III FLASH1M.
    mgba_flash_runahead_guard: bool,
    sma4_runahead_guard: bool,
    palette_live: bool,
    core_kind: Core,
    pending_undo: Option<(Instant, PendingUndo)>,
    last_sram: Instant,
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

    /// Reopen a *fresh* two-GBA mGBA instance. The libretro link options
    /// must be set BEFORE retro_load_game; an existing solo core cannot switch.
    /// Never pass RetroAchievements to this experimental dual-instance session.
    pub fn open_mgba_linked(rom: &Path, core_file: &Path, storage: &Path,
                            rom_stem: &str, colour_correction: bool,
                            role: Role, solo_state: &[u8],
                            peer_rom: Option<&Path>) -> Result<Self, String> {
        // Fail BEFORE any filesystem writes or a dropped solo core if the
        // source state cannot fit as a valid single-GBA Link seed.
        crate::linked_storage::validate_solo_snapshot(solo_state)?;
        Self::open_internal(rom, core_file, storage, Core::Mgba, Platform::Gba,
            false, rom_stem, None, colour_correction, None, Some(role), "auto",
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
        if linked_boot.is_some() && (platform != Platform::Gba ||
            resume_state || achievements.is_some()) ||
            (linked_boot.is_some() != linked_resume.is_some()) {
            return Err("Invalid isolated Multiplayer boot configuration".into());
        }
        if peer_rom.is_some() && (linked_boot.is_none() || which != Core::Mgba) {
            return Err("A remote GBA cartridge is only valid inside mGBA Cable Link".into());
        }
        if let Some(peer) = peer_rom {
            if !peer.is_file() || peer.extension().and_then(|v|v.to_str()) != Some("gba")
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
        let mut core = LibretroCore::open_with(core_file, &bios, &core_root)
            .map_err(|e| e.to_string())?;
        // Exact upstream Slot libretro options for colour and Game Boy palettes.
        // Read only the 336-byte header: never read the entire cached ROM.
        let dmg_only = platform != Platform::Gba && which == Core::Mgba
            && std::fs::File::open(rom).and_then(|mut file| {
                let mut header = [0u8; 0x150];
                file.read_exact(&mut header)?;
                Ok(header[0x143] & 0x80 == 0)
            }).unwrap_or(false);
        let palette_live = dmg_only && gb_palette.is_some();
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
                core.set_option("mgba_force_gbp", "ON");
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
        }
        // rcheevos shares ONE evaluator, protected against cross-console
        // memory reads. GB=4 / GBA=5 / GBC=6 use their own libretro maps.
        // Speculative frames never reach it (including GB/GBC Run-Ahead).
        let console = match platform {
            Platform::Gb => 4, Platform::Gba => 5, Platform::Gbc => 6,
        };
        let mut core: Box<dyn RetroCore> = match achievements {
            Some(service) => service.wrap_for_console(Box::new(core), console),
            None => Box::new(core),
        };
        core.load(rom).map_err(|e| e.to_string())?;
        let load_ms = opening_at.elapsed().as_millis();
        let restore_started = Instant::now();
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
        if resume_state && linked_boot.is_none() && !cross_core_import {
            // Prefer a successfully decoded RetroArch auto state, then the
            // Slot private state. A compressed/unrecognized external file is
            // kept intact and is NEVER interpreted as raw emulator memory.
            let external = std::fs::read(&retroarch_import)
                .ok().and_then(|bytes| match retroarch_state::decode(&bytes) {
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
        Ok(Self {
            core, link: None, link_touched: false, linked_boot, gpsp_serial,
            rom_path: rom.to_path_buf(), rom_stem: rom_stem.to_owned(),
            save, rtc, manual_export_prefix, state, retroarch_export, retroarch_import, ring,
            rewind: RewindThread::spawn(REWIND_BYTES), rewind_frames: 0,
            frame_count: 0, palette_live, core_kind: which,
            runahead_video: Vec::new(), runahead_state: Vec::new(),
            runahead_unavailable: false,
            mgba_flash_runahead_guard: flash_guard,
            sma4_runahead_guard: sma4_guard,
            pending_undo: None, last_sram: Instant::now(),
            sample_rate: sample_rate.clamp(8_000, 96_000),
            fps,
        })
    }

    /// False while cable lockstep is awaiting the other player. The caller
    /// must not subtract an emulated-frame budget until this returns true.
    pub fn advance(&mut self, buttons: u16, turbo: bool, record_rewind: bool) -> bool {
        // A suspended or ended Multiplayer session must not fall through
        // to core.run_frame() merely because self.link became None. This
        // also handles Android onPause outside the Multiplayer overlay.
        if !emulation_may_advance(self.link_touched, self.link.is_some()) {
            return false;
        }
        self.runahead_video.clear();
        let pressed = ButtonMask(buttons);
        let mapped = xy_buttons(pressed, turbo, self.frame_count);
        if let Some(link) = self.link.as_mut() {
            match link.frame(self.core.as_mut(), mapped) {
                LinkFrame::Advanced => {},
                LinkFrame::Waiting | LinkFrame::Failed(_) => return false,
            }
        } else {
            self.core.run_frame(mapped);
        }
        self.frame_count = self.frame_count.wrapping_add(1);
        if record_rewind && !self.link_touched {
            self.rewind_frames += 1;
            if self.rewind_frames >= REWIND_CAPTURE_FRAMES {
                self.rewind_frames = 0;
                // Original XOR/LZ4 history; never export it to RetroArch.
                if let Ok(snapshot) = self.core.serialize() {
                    self.rewind.push(snapshot);
                }
            }
        } else { self.rewind_frames = 0; }
        if self.last_sram.elapsed() >= SRAM_PERIOD {
            self.save_sram();
        }
        true
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
        }
        let link = LinkPlay::begin(self.core.as_mut(), role, mode, transport)?;
        self.link = Some(link);
        self.link_touched = true;
        self.runahead_video.clear();
        self.rewind_frames = 0;
        self.pending_undo = None;
        Ok(())
    }

    pub fn link_fault(&self) -> Option<LinkError> {
        self.link.as_ref().and_then(|link| link.fault())
    }

    pub fn linked(&self) -> bool { self.link.is_some() }

    /// Keep the connected gameplay session alive while a native overlay
    /// is open. The peer stops at its next emulation callback without
    /// sending any artificial GBA serial packets.
    pub fn set_link_paused(&mut self, paused: bool) {
        if let Some(link) = self.link.as_mut() { link.set_local_paused(paused); }
    }

    pub fn link_peer_paused(&self) -> bool {
        self.link.as_ref().is_some_and(|link| link.peer_paused())
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
        restore_solo_checkpoint_to_core(self.core.as_mut(), saved)?;
        // Clear presentation/input time travel from any pre-recovery state.
        self.runahead_video.clear();
        self.rewind_frames = 0;
        self.pending_undo = None;
        Ok(())
    }

    /// Save the solo state before replacing the entire libretro core with
    /// the linked dual-GBA engine. Abort rather than discard solo progress.
    pub fn preserve_solo_for_link(&mut self) -> Result<Vec<u8>, String> {
        if self.linked_boot.is_some() || self.link_touched {
            return Err("Only a clean solo game can start mGBA Link".into());
        }
        // A13: never seed a Pokémon Cable session from a malformed FLASH1M
        // image. The in-memory core can differ from the verified disk save,
        // so check it BEFORE writing the checkpoint or dropping the solo core.
        let battery = self.core.save_ram();
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
        let ram = self.core.save_ram();
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
        self.ensure_pokemon_flash(self.core.save_ram().as_deref(), "before promotion")?;
        let resumed = self.core.serialize()
            .map_err(|e| format!("Cannot verify resumed solo state: {e}"))?;
        crate::linked_storage::validate_solo_snapshot(&resumed)?;
        if let Some(expected) = rejoin.ram.as_deref() {
            if self.core.save_ram().as_deref() != Some(expected) {
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
        self.runahead_video.clear();
        self.rewind_frames = 0;
    }

    pub fn mgba_flash_runahead_blocked(&self) -> bool { self.mgba_flash_runahead_guard }
    pub fn sma4_runahead_blocked(&self) -> bool { self.sma4_runahead_guard }

    pub fn runahead_supported(&self) -> bool {
        !self.mgba_flash_runahead_guard && !self.sma4_runahead_guard
            && self.linked_boot.is_none() && !self.link_touched
            && self.link.is_none() && !self.runahead_unavailable
            && self.core.speculative_audio_supported()
    }

    /// Roll back both standard libretro state and core-specific transient
    /// audio: gpSP's sound ring, or mGBA's converter/filter/queue sidecar.
    pub fn prepare_runahead(&mut self, buttons: u16, turbo: bool, frames: u8) {
        self.runahead_video.clear();
        if frames == 0 || frames > 2 || self.mgba_flash_runahead_guard ||
            self.sma4_runahead_guard ||
            self.runahead_unavailable || self.link_touched || self.linked_boot.is_some() { return; }
        if !self.core.speculative_audio_supported() {
            self.runahead_unavailable = true;
            log_launch_timing("Run-Ahead disabled: core has no safe speculative audio support");
            return;
        }
        if let Err(error) = self.core.serialize_into(&mut self.runahead_state) {
            self.runahead_unavailable = true;
            log_launch_timing(&format!("Run-Ahead disabled: serialize failed: {error}"));
            return;
        }
        if let Err(error) = self.core.capture_speculative_audio() {
            self.runahead_unavailable = true;
            log_launch_timing(&format!("Run-Ahead disabled: audio snapshot failed: {error}"));
            return;
        }
        let pressed = ButtonMask(buttons);
        let mapped = xy_buttons(pressed, turbo, self.frame_count);
        for _ in 0..frames {
            self.core.run_speculative_frame(mapped);
            // Discard predicted audio without reallocating libretro's buffer.
            self.core.discard_audio();
        }
        self.runahead_video.extend_from_slice(self.core.video_xrgb8888());
        let restored = self.core.restore_speculative_state(&self.runahead_state)
            .and_then(|_| self.core.restore_speculative_audio());
        if let Err(error) = restored {
            // Never display a frame if the core could not restore its real state.
            self.runahead_video.clear();
            self.runahead_unavailable = true;
            log_launch_timing(&format!("Run-Ahead disabled: restore failed: {error}"));
            return;
        }

    }

    pub fn rewind_step(&mut self) -> bool {
        if self.link_touched || self.linked_boot.is_some() { return false; }
        self.runahead_video.clear();
        let Some(previous) = self.rewind.pop() else { return false; };
        if self.core.unserialize(&previous).is_err() { return false; }
        // Most libretro cores only refresh video after retro_run. Run a single
        // input-free frame so the renderer shows the restored moment.
        self.core.run_frame(ButtonMask(0));
        let _ = self.core.take_audio();
        self.rewind_frames = 0;
        true
    }

    pub fn can_change_gb_palette(&self) -> bool {
        self.palette_live
    }

    pub fn set_gb_palette(&mut self, name: &str) {
        if self.palette_live {
            self.core.set_option("mgba_gb_colors_preset", "0");
            self.core.set_option("mgba_gb_colors", name);
        }
    }

    /// Per-game GB palette enable/disable is applied through mGBA's live core
    /// options, without restarting libretro or touching ROM SRAM/RTC.
    pub fn set_gb_palette_mode(&mut self, enabled: bool, name: &str) {
        if !self.palette_live { return; }
        self.core.set_option("mgba_gb_colors_preset", if enabled { "0" } else { "1" });
        if enabled { self.core.set_option("mgba_gb_colors", name); }
    }

    pub fn set_colour_correction(&mut self, enabled: bool) {
        let (key, value) = match self.core_kind {
            Core::Mgba => ("mgba_color_correction",
                if enabled { "Auto" } else { "OFF" }),
            Core::Gpsp => ("gpsp_color_correction",
                if enabled { "enabled" } else { "disabled" }),
        };
        self.core.set_option(key, value);
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

    fn save_sram(&mut self) -> bool {
        let success = match self.core.save_ram() {
            Some(bytes) if self.mgba_flash_runahead_guard &&
                pokemon_flash_complete_bank_count(&bytes) == 0 => {
                // A Flash erase/program operation can span several GBA
                // frames. Never publish an incomplete battery image over
                // the last known-good save or push it into RetroArch SAF.
                // This does not modify emulator memory and never repairs
                // corruption silently: the in-game error remains visible.
                log_launch_timing("mGBA Pokemon FLASH1M: incomplete/corrupt bank; protected existing SRAM, no disk export");
                false
            }
            Some(bytes) => match atomic_write(&self.save, &bytes) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("slot-konkr: SRAM save failed: {err}");
                    false
                }
            },
            None => false,
        };
        // RTC may exist independently of SRAM on GB/GBC cartridges.
        let rtc_saved = self.core.save_rtc().is_some_and(|bytes|
            match atomic_write(&self.rtc, &bytes) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("slot-konkr: RTC save failed: {err}");
                    false
                }
            });
        self.last_sram = Instant::now();
        success || rtc_saved
    }

    pub fn save(&mut self, with_state: bool) -> bool {
        let saved_sram = self.save_sram();
        let mut state_written = false;
        // Exporting a single-core state during linked play would silently
        // corrupt the shared timeline. SRAM/RTC can still be saved safely.
        if with_state && !self.link_touched && self.linked_boot.is_none() {
            if let Ok(data) = self.core.serialize() {
                if let Err(err) = atomic_write(&self.state, &data) {
                    eprintln!("slot-konkr: state save failed: {err}");
                }
                match retroarch_state::encode(&data) {
                    Ok(container) => match atomic_write(&self.retroarch_export, &container) {
                        Ok(()) => state_written = true,
                        Err(error) => eprintln!("slot-konkr: RetroArch state export failed: {error}"),
                    },
                    Err(error) => eprintln!("slot-konkr: RetroArch container error: {error}"),
                }
            }
        }
        saved_sram || state_written
    }
    /// Original Slot ring: 10 timestamped states per ROM/core, each with
    /// a 240×160 PNG thumbnail. Independent from RetroArch's manual slots.
    pub fn save_manual(&mut self) -> Result<String, String> {
        if self.link_touched || self.linked_boot.is_some() { return Err("Save States unavailable for Multiplayer sessions".into()); }
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
        let encoded = retroarch_state::encode(&state)?;
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
        if self.link_touched || self.linked_boot.is_some() { return Err("Load State unavailable during Multiplayer".into()); }
        let (bytes, _) = self.ring.read(stamp).map_err(|e|e.to_string())?;
        let prior = self.core.serialize().map_err(|e|e.to_string())?;
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
        if self.link_touched || self.linked_boot.is_some() { return Err("Save States unavailable during Multiplayer".into()); }
        self.ring.remove(stamp).map_err(|e|e.to_string())
    }

    pub fn undo_manual(&mut self)->Result<(),String> {
        if self.link_touched || self.linked_boot.is_some() { return Err("Undo unavailable during Multiplayer".into()); }
        let Some((since,_))=&self.pending_undo else { return Err("Nothing to undo".into())};
        if since.elapsed()>UNDO_WINDOW {
            self.pending_undo=None;
            return Err("Undo expired (30 seconds)".into());
        }
        let (_, pending)=self.pending_undo.take().unwrap();
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
        self.linked_boot.is_none() && !self.link_touched && self.pending_undo.as_ref().is_some_and(|(at,_)|at.elapsed()<=UNDO_WINDOW)
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
