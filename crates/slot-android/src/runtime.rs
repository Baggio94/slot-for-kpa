use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use crate::game::GameSession;
use crate::settings::Settings;
use crate::emulation::EmulationProfile;
use std::time::Duration;
use crate::video_mode::{self, VideoMode};
use crate::core_selection;
use crate::cart_render::CartRender;
use crate::core_picker::{CorePicker, Press};
use slot_retro::ButtonMask;
use jni::{JNIEnv, objects::{JObject, JString}, sys::{jint, jboolean, jshortArray, jstring}};
use std::sync::Mutex;
use std::time::Instant;
use std::net::Ipv4Addr;
use crate::multiplayer::{LinkStarter, Progress as LinkProgress, Role as LinkRole};
use crate::link_kind::{self, LinkKind};
use crate::app::{GameMenu as LinkMenu, LinkRow, LinkFail, LinkStep};
use crate::link_screen::{self, LinkSprites, Sprite};

use slot_gfx::{Compositor, GfxError, Surface, OUT_H, OUT_W};
use slot_store::{Cart, Platform, Core, stamp_now};
use slot_power::{Battery, Charge};
use crate::library::{carts_by_platform, RomEntry, LIBRARY};
use slot_ui::{
    board_face, chip_face, chip_shadow_face, socket_face, quick_value_face, cart_face_with_material,
    cart_shadow, gb_cart_shadow, board_from, board_zoom, lift_of, shelf_cart_at,
    on_board, lid_from, grown, draw_empty_slot, Draw, GbShell, Shelf, SlotChrome,
    BOARD_X, BOARD_W, SOCKET_U, SOCKET_V, SOCKET_W, SOCKET_H,
    CHIP_U, CHIP_V, CHIP_W, CHIP_H, HOP_LIFT, TURN_PAD, SHADOW_W, SHADOW_H,
    CART_W, hint_face, arrows_hint_face, title_face, photo_face, Polaroids, Printed, HINT_H, HINT_EDGE,
    draw_footer, draw_slot_name, draw_platform_name, word_face, platform_name_face,
    icon_face, badge_face, menu_face, Icon, LinkBadge, BOLT_PX, HUD_INK,
    Hud, HudKind, FfState, Toast, toast_face, QUICK_PITCH, opening, edge, centred_hints, LEGEND_GAP,
    sticker_face_konkr, StickerFields, STICKER_W, STICKER_H, quick_caret_face,
    onboarding_instruction_face, onboarding_note_face,
};

// Android GLSurfaceView owns EGL context creation, current context and buffer swaps.
#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
}

struct AndroidSurface {
    size: (u32, u32),
}

impl Surface for AndroidSurface {
    fn make_current(&mut self) -> Result<(), GfxError> {
        Ok(())
    }

    fn window_size(&self) -> (u32, u32) {
        self.size
    }

    fn swap(&mut self) -> Result<(), GfxError> {
        Ok(())
    }

    fn proc_address(&self, name: &str) -> *const c_void {
        CString::new(name)
            .map(|c| unsafe { eglGetProcAddress(c.as_ptr()) })
            .unwrap_or(std::ptr::null())
    }
}

#[derive(Clone)]
enum Input {
    Key(i32, bool),
    // Predefined shelf UI states from Android: no untrusted or long text on GL.
    ShelfStatus(u8),
    Reset,
    GameReady { uri: String, path: String },
    GameError { uri: String, message: String },
    Exit,
    Suspend,
    ReloadVisuals,
    ReloadCartLabel { uri: String },
    // Android announces network return; only wake the HTTP retry timer.
    // Never infer RA API authentication/reachability from Wi-Fi alone.
    RaNetworkAvailable,
    RaAccountConfigured,
    // Kotlin acquires the multicast lock before returning a Wi-Fi IPv4.
    LinkNetworkReady(String),
    LinkNetworkError(String),
    LinkPeerRomConsent(i32),
    RaPrefetchHash(String, u32),
    ExternalReturn,
}

static INPUT: Mutex<VecDeque<Input>> = Mutex::new(VecDeque::new());
static REQUEST: Mutex<Option<String>> = Mutex::new(None);
static LABEL_PICK_REQUEST: Mutex<Option<String>> = Mutex::new(None);
static UI_ACTION: Mutex<VecDeque<i32>> = Mutex::new(VecDeque::new());
static SAVE_SYNC: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static SHARE_SAVES_ENABLED: AtomicBool = AtomicBool::new(false);
// -1 = preserve ordinary Slot rule, 0 = start from SRAM, 1 = user chose
// existing destination-core state in the explicit Share Saves dialogue.
static NEXT_RESUME_MODE: AtomicI32 = AtomicI32::new(-1);
static CART_SFX: Mutex<VecDeque<i32>> = Mutex::new(VecDeque::new());
const CART_INSERT_SFX: i32 = 1;
const CART_EJECT_SFX: i32 = 2;
// Exact upstream Slot timeline: 450ms mechanical seating + 280ms hold.
const INSERT_S: f32 = 0.73;
const SEATED_AT: f32 = 0.45;
// Original Slot starts the 48-kHz insert clip 97ms before the cart seats,
// so the recorded contact lands at 450ms, not 143ms early.
const INSERT_SOUND_LEAD_S: f32 = 0.097;
const INSERT_SOUND_PROGRESS: f32 = (SEATED_AT - INSERT_SOUND_LEAD_S) / SEATED_AT;
// Upstream Slot screen power transition times (app.rs).
const SCREEN_POWER_ON_S: f32 = 0.22;
const SCREEN_POWER_OFF_S: f32 = 0.16;
const EJECT_S: f32 = 0.45;
const EJECT_HOLD_S: f32 = 0.35; // Original Slot hold after CRT-off.
// Slightly later than upstream's first-motion eject click, by request:
// retain the original 160ms CRT + 350ms hold + 450ms travel unchanged.
const EJECT_SOUND_DELAY_S: f32 = 0.040;
const EJECT_SOUND_PROGRESS: f32 = 1.0 - EJECT_SOUND_DELAY_S / EJECT_S;
const SELECT_CHORD_MS: u128 = 600;
const SELECT_TAP_FRAMES: u8 = 3;
const SHELF_STATUS_TEXT: [&str; 8] = [
    "", "SCANNING CARTRIDGES...", "NO GAMES FOUND",
    "LIBRARY IMPORT FAILED", "FOLDER ACCESS EXPIRED",
    "CANNOT READ ROM FOLDER", "FOLDER PICKER UNAVAILABLE",
    "FOLDER PERMISSION FAILED",
];
const REWIND_STEP_S: f32 = 0.10;  // Original Slot's time-travel HUD + ~10 Hz restoration.

const FF_DOUBLE_TAP_MS: u128 = 320;
static MESSAGE: Mutex<Option<String>> = Mutex::new(None);
// RA worker runs off the GL thread. The engine samples its nonblocking status
// and forwards bounded, sanitized user-facing notices to Android's main thread.
static RA_NOTICES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static RA_STATUS: Mutex<Option<String>> = Mutex::new(None);
const RA_NOTICE_LIMIT: usize = 8;
// Up to 96 prepared cartridge faces (~70 MiB worst-case native RGBA artwork).
// KONKR has 4 GiB shared RAM; the LRU still reuses existing GLES textures.
const CART_TEXTURE_CACHE_CAPACITY: usize = 160;
const CART_IDLE_PREFETCH_CAP: usize = 155;
const CART_IDLE_GRACE: Duration = Duration::from_millis(400);
const CART_GPU_UPLOAD_BUDGET: Duration = Duration::from_millis(2);

fn ra_notice_payload(title: &str, detail: &str, earned: bool, badge: Option<&Path>) -> String {
    let clean = |value: &str, limit: usize| -> String {
        value.chars().filter(|ch| !ch.is_control()).take(limit).collect()
    };
    // BadgeName is validated by slot_achievements::badges::path before being
    // attached to Notice. Android must still verify canonical app-private path
    // before reading the image. Empty means fall back to a Slot glyph.
    serde_json::json!({
        "title": clean(title, 100),
        "detail": clean(detail, 160),
        "earned": earned,
        "badge": badge.and_then(|path| path.to_str()).unwrap_or(""),
    }).to_string()
}

fn queue_ra_notice(queue: &mut VecDeque<String>, payload: String) {
    if queue.len() >= RA_NOTICE_LIMIT { queue.pop_front(); }
    queue.push_back(payload);
}

static PATHS: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);
static AUDIO: Mutex<VecDeque<i16>> = Mutex::new(VecDeque::new());
static PLAYING: AtomicBool = AtomicBool::new(false);
static SAMPLE_RATE: AtomicI32 = AtomicI32::new(0);
static DISPLAY_SYNC: AtomicBool = AtomicBool::new(false);
// Updated by the GL owner after an entirely settled, fully preloaded shelf frame.
// Kotlin may switch the idle carousel to requestRender without affecting games.
static CAROUSEL_QUIET: AtomicBool = AtomicBool::new(false);
// The GLES owner reports a stable selection, never dereferencing SAF URIs itself.
static WARM_ROM_URI: Mutex<Option<String>> = Mutex::new(None);
// Read-only GL-owned library swap gate for Kotlin's background scanner.
static LIBRARY_UPDATE_SAFE: AtomicBool = AtomicBool::new(false);
// A12 diagnostic A/B. Default keeps A11 recovery; flag file selects legacy.
static MGBA_CABLE_RECOVERY: AtomicBool = AtomicBool::new(true);
// Android AudioTrack time-stretches 2x game audio while preserving pitch.
// Expose the current transport speed separately from libretro's sample rate.
static AUDIO_SPEED_PERMILLE: AtomicI32 = AtomicI32::new(1000);
static FF_AUDIO_SUPPORTED: AtomicBool = AtomicBool::new(true);
static FF_AUDIO_ENABLED: AtomicBool = AtomicBool::new(true);
static RUMBLE_STRENGTH: AtomicI32 = AtomicI32::new(0);
// Updated once per second by the GL thread; read-only and optional on Android.
static PERF_STATUS: Mutex<Option<String>> = Mutex::new(None);
static PERF_ACTIVE: AtomicBool = AtomicBool::new(false);
// Read-only status bar snapshot for Android's upper shelf overlays.
// Updated on the GL thread alongside the *existing* Slot footer gate.
static SHELF_INDICATOR_FLAGS: AtomicI32 = AtomicI32::new(0);
// Read-only lifecycle bit polled by Kotlin; never touch WifiManager on GL.
static LINK_NEEDS_MULTICAST: AtomicBool = AtomicBool::new(false);
// Authenticated peer key fingerprint, never a user-supplied device name.
static LINK_PEER_LABEL: Mutex<Option<String>> = Mutex::new(None);

#[derive(Clone)]
struct SystemStatus {
    clock: String,
    battery: Option<Battery>,
}
static SYSTEM_STATUS: Mutex<SystemStatus> = Mutex::new(SystemStatus {
    clock: String::new(), battery: None,
});

fn controls(code: i32) -> u16 {
    match code {
        97 => ButtonMask::B, 100 => ButtonMask::Y, 109 => ButtonMask::SELECT,
        108 => ButtonMask::START, 19 => ButtonMask::UP, 20 => ButtonMask::DOWN,
        21 => ButtonMask::LEFT, 22 => ButtonMask::RIGHT,
        96 => ButtonMask::A, 99 => ButtonMask::X,
        102 => ButtonMask::L, 103 => ButtonMask::R,
        _ => 0,
    }
}

// A consumed SELECT chord must also consume that key's release.
fn select_chord_bit(code: i32) -> u16 {
    match code {
        96 => 1, 97 => 2, 100 => 4, 102 => 8,
        103 => 16, 104 => 32, 105 => 64,
        _ => 0,
    }
}

/// Upstream slot-input: SELECT held, then HOME/MODE opens the Link menu.
/// Consume both the press and its release, otherwise HOME also opens the
/// regular game menu (or cancels the Link picker on release).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SelectHomeAction { Pass, OpenLink, Swallow }

fn select_home_action(
    pressed: bool,
    select_held: bool,
    in_game: bool,
    may_open: bool,
    home_consumed: &mut bool,
) -> SelectHomeAction {
    if *home_consumed {
        if !pressed { *home_consumed = false; }
        return SelectHomeAction::Swallow;
    }
    if pressed && select_held && in_game && may_open {
        *home_consumed = true;
        return SelectHomeAction::OpenLink;
    }
    SelectHomeAction::Pass
}

fn set_message(message: String) {
    *MESSAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
}


// A12 GL-thread-only diagnostic; no extra simulation step or GPU readback.
// Native draw duration is measured BEFORE GLSurfaceView's EGL buffer swap.
struct A12DrawProbe {
    started: Instant,
    callbacks: u64,
    steps0: u64,
    steps1: u64,
    steps2: u64,
    native_us_max: u64,
    over_16ms: u64,
    over_33ms: u64,
}
impl A12DrawProbe {
    fn new() -> Self {
        Self { started: Instant::now(), callbacks: 0, steps0: 0,
            steps1: 0, steps2: 0, native_us_max: 0,
            over_16ms: 0, over_33ms: 0 }
    }
    fn record(&mut self, steps: usize, us: u64, recovery: bool) {
        self.callbacks += 1;
        match steps {
            0 => self.steps0 += 1,
            1 => self.steps1 += 1,
            _ => self.steps2 += 1,
        }
        self.native_us_max = self.native_us_max.max(us);
        if us > 16_667 { self.over_16ms += 1; }
        if us > 33_333 { self.over_33ms += 1; }
        let span = self.started.elapsed();
        if span >= Duration::from_secs(5) {
            crate::game::log_launch_timing(&format!(
                "Link display pacing: recovery={} callbacks={} emu_fps={:.1} steps0={} steps1={} steps2={} native_us_max={} native_over_16ms={} native_over_33ms={}",
                if recovery { "a11" } else { "legacy" }, self.callbacks,
                (self.steps1 + 2 * self.steps2) as f64 / span.as_secs_f64(),
                self.steps0, self.steps1, self.steps2, self.native_us_max,
                self.over_16ms, self.over_33ms));
            *self = Self::new();
        }
    }
}
thread_local! {
    static A12_DRAW_PROBE: RefCell<Option<A12DrawProbe>> = const { RefCell::new(None) };
}
fn a12_draw_sample(linked: bool, steps: usize, native_us: u64, recovery: bool) {
    A12_DRAW_PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        if linked {
            probe.get_or_insert_with(A12DrawProbe::new).record(steps, native_us, recovery);
        } else {
            *probe = None;
        }
    });
}

// All engine / OpenGL work occurs on GLSurfaceView's dedicated GL thread.
thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum LinkEntry { Shortcut, GameMenu }

impl LinkEntry {
    fn back(self, linked: bool, requires_relaunch: bool) -> ShelfOverlay {
        match self {
            Self::Shortcut => ShelfOverlay::None,
            Self::GameMenu => ShelfOverlay::GameMenu {
                row: pause_rows(linked, requires_relaunch).iter()
                    .position(|item| *item == PauseRow::Multiplayer).unwrap_or(0),
            },
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ShelfOverlay {
    None,
    Menu { row: usize },
    Library { row: usize },
    Settings { row: usize },
    ScreenSettings { row: usize },
    ScreenSystemSettings { platform: Platform, row: usize },
    GameplaySettings { row: usize },
    EmulationSettings { row: usize },
    InterfaceSettings { row: usize },
    GeneralSettings { row: usize },
    // Pause submenus never mutate the global Settings object.
    PauseSettings { row: usize },
    PauseScreenSettings { row: usize },
    PauseGameplaySettings { row: usize },
    PauseEmulationSettings { row: usize },
    About,
    Updates { row: usize },
    Scraping,
    Achievements,
    Core,
    GameMenu { row: usize },
    Multiplayer,
    States,
}

const MENU_TEXT: [&str; 94] = [
    "MENU",                             // 0
    "Library",                          // 1
    "Cart Studio",                      // 2
    "RetroAchievements",               // 3
    "A Select     B Back",              // 4
    "LIBRARY",                          // 5
    "Choose ROM Folder",               // 6
    "Choose Save Folder",              // 7
    "Choose Save State Folder",        // 8
    "Refresh Library",                 // 9
    "RetroArch core folders",           // 10
    "CART STUDIO",                      // 11
    "A: Open Cart Studio",             // 12
    "RETROACHIEVEMENTS",               // 13
    "A: RA account / sign in",         // 14
    "SELECT CORE",                      // 15
    "mGBA",                             // 16
    "gpSP",                            // 17
    "A Confirm     B Back",             // 18
    "GAME MENU",                        // 19
    "Resume",                           // 20
    "Save and Eject",                   // 21
    "Save States",                      // 22
    "A Select     B Back",              // 23
    "Choose BIOS Folder",               // 24
    "Settings",                         // 25
    "SETTINGS",                         // 26
    "Fast Forward",                     // 27
    "Fast Forward Sound",               // 28
    "Color Correction",                 // 29
    "Rumble",                           // 30
    "About",                            // 31
    "SCREEN",                           // 32
    "GAMEPLAY",                         // 33
    "GBA Shader",                       // 34
    "GB / GBC Shader",                  // 35
    "GB Palettes",                      // 36
    "Rewind",                           // 37
    "X/Y Buttons",                       // 38
    "Auto Save on Eject",               // 39
    "",                                 // 40 retired
    "",                                 // 41 retired
    "",                                 // 42 retired
    "Import Wallpaper",                 // 43
    "",                                 // 44 retired
    "Delete Wallpaper",                 // 45
    "",                                 // 46 retired
    "",                                 // 47 retired
    "Apps",                             // 48
    "Emulation",                        // 49
    "EMULATION",                        // 50
    "Show Stats",                       // 51
    "Frame pacing",                     // 52
    "STANDARD",                         // 53
    "RESPONSIVE",                       // 54
    "Run-Ahead",                        // 55
    "GAME EMULATION",                   // 56
    "Reset to global",                  // 57
    "Interface",                        // 58
    "INTERFACE",                        // 59
    "Wi-Fi",                            // 60
    "Bluetooth",                        // 61
    "App Integrations",                 // 62 legacy
    "Show Wi-Fi icon",                  // 63
    "Show Bluetooth icon",             // 64
    "Show app status icons",            // 65
    "Multiplayer",                      // 66
    "End Link",                         // 67
    "",                                 // 68 retired: old non-Slot Multiplayer title
    "",                                 // 69 retired: LinkRow::text() is upstream
    "",                                 // 70 retired: LinkRow::text() is upstream
    "",                                 // 71 retired: no extraneous mode heading
    "",                                 // 72 retired: no extraneous mode heading
    "",                                 // 73 retired: LinkStep::line() is upstream
    "",                                 // 74 retired: upstream says Linked
    "",                                 // 75 retired: LinkFail::line() is upstream
    "",                                 // 76 retired: no middle-page helper text
    "",                                 // 77 retired: upstream shows A OK
    "Eject to play solo",               // 78
    "Share mGBA/gpSP Saves",            // 79
    "",                                  // 80 retired: value rendered at right
    "General",                            // 81, official Slot v1.7 menu
    "GENERAL",                            // 82
    "Wallpaper",                          // 83
    "Wallpaper Tint",                     // 84
    "Theme",                              // 85
    "Recents",                            // 86
    "Game Titles",                        // 87
    "GBC Shader",                         // 88
    "GB Shader",                          // 89
    "Bezel",                              // 90
    "Game Boy Advance",                   // 91
    "Game Boy Color",                     // 92
    "Game Boy",                           // 93
];

#[derive(Clone, Copy)]
struct LiveGameOptions {
    ff_speed: u8,
    ff_sound: bool,
    rewind: bool,
    turbo: bool, // KONKR X/Y Buttons
    rumble: bool,
    eject_save: bool,
}
impl LiveGameOptions {
    fn from_profile(global: &crate::settings::Settings, p: Option<EmulationProfile>) -> Self {
        let ff_speed=p.and_then(|v|v.ff_speed_override).unwrap_or(global.ff_speed);
        Self {
            ff_speed,
            ff_sound:p.and_then(|v|v.ff_sound_override).unwrap_or(global.ff_sound),
            rewind:p.and_then(|v|v.rewind_override).unwrap_or(global.rewind),
            turbo:p.and_then(|v|v.turbo_override).unwrap_or(global.turbo),
            rumble:p.and_then(|v|v.rumble_override).unwrap_or(global.rumble),
            eject_save:p.and_then(|v|v.eject_save_override).unwrap_or(global.eject_save),
        }
    }
}

#[derive(Clone,Copy)]
struct ClearMaterial {
    key:(usize,usize),
    tex:[slot_gfx::TexId;3], // official back -> PCB -> plastic front
}

struct Engine {
    gpu: Compositor,
    size: (u32, u32),
    shelves: Vec<Shelf>,
    active: usize,
    inserted: bool,
    progress: f32,
    // Original slot. 300ms/6px refusal shake; shelf visuals only.
    refusal: Option<slot_ui::Refusal>,
    insert_started: Option<Instant>,
    screen_power: f32,
    exiting_screen: bool,
    eject_hold_remaining: f32,
    select_down_at: Option<Instant>,
    // HOME release must not close the Link screen opened by SELECT + HOME.
    select_home_consumed: bool,
    select_chorded: bool,
    select_forwarded: bool,
    select_tap_frames: u8,
    chord_releases: u16,
    born: Instant,
    last: Instant,
    library_version: u64,
    texture_cache: VecDeque<(usize, usize, slot_gfx::TexId)>,
    last_cart_navigation: Instant,
    cart_render: CartRender,
    clear_material: Option<ClearMaterial>,
    game: Option<GameSession>,
    pending_game: Option<GameSession>,
    link_identity: Option<crate::link_rom::GbaIdentity>,
    link_role: LinkRow,
    link_mode: LinkKind,
    // Optional mode chosen from original Slot's SELECT Mode key. Reset on
    // each new cartridge; keep it when returning from a cancelled link menu.
    link_mode_override: Option<LinkKind>,
    link_phase: LinkMenu,
    link_entry: LinkEntry,
    link_worked_at: u64,
    link_starter: Option<LinkStarter>,
    link_art: LinkSprites,
    // A single account-scoped engine outlives individual games, allowing
    // queued offline awards to sync even after the cartridge is ejected.
    ra_service: Option<slot_achievements::Service>,
    ra_signing_out: bool,
    buttons: u16,
    ff_held: bool,
    ff_latched: bool,
    ff_last_release: Option<Instant>,
    rewind_held: bool,
    rewind_accum: f32,
    awaiting_game: bool,
    requested_uri: Option<String>,
    prepared_rom: Option<String>,
    game_accum: f64,
    perf_last: Instant,
    perf_rendered: u32,
    perf_emulated: u32,
    perf_catchup: u32,
    external_pending: bool,
    overlay: ShelfOverlay,
    settings: Settings,
    game_emulation: Option<EmulationProfile>,
    emulation_core_labels: [(slot_gfx::TexId, u32, u32); 2],
    video_mode: VideoMode,
    wallpaper_texture: Option<slot_gfx::TexId>,
    wallpaper_personal: bool,
    wallpaper_name_face: Option<(slot_gfx::TexId, u32, u32)>,
    /// Live rear/front v1.7.0 slot textures. Always draw in proper depth order.
    slot_faces: Option<(slot_gfx::TexId, slot_gfx::TexId)>,
    slot_dark_faces: (slot_gfx::TexId, slot_gfx::TexId),
    slot_light_faces: (slot_gfx::TexId, slot_gfx::TexId),
    gb_bezel_tex: slot_gfx::TexId,
    gbc_bezel_tex: slot_gfx::TexId,
    custom_wallpaper_present: bool,
    setting_values: Vec<(slot_gfx::TexId, u32, u32)>,
    setting_carets: [(slot_gfx::TexId, u32, u32); 2],
    settings_hints: [(slot_gfx::TexId, u32); 2],
    about_texture: Option<slot_gfx::TexId>,
    menu_seen: u8,
    menu_opened: Instant,
    menu_cursor_y: f32,
    menu_textures: Vec<(slot_gfx::TexId, u32, u32)>,
    menu_hints: [(slot_gfx::TexId, u32); 2],
    first_run_hint: (slot_gfx::TexId, u32),
    onboarding_note: (slot_gfx::TexId, u32),
    shelf_no_games_hint: (slot_gfx::TexId, u32),
    shelf_status: u8,
    shelf_status_faces: [(slot_gfx::TexId, u32, u32); 8],
    shelf_status_start: (slot_gfx::TexId, u32),
    game_menu_hints: [(slot_gfx::TexId, u32); 3],
    // Same centered role, progress and failure faces as original slot.
    link_role_faces: [(slot_gfx::TexId, u32, u32); 2],
    link_step_faces: [(slot_gfx::TexId, u32, u32); 2],
    link_fail_faces: [(slot_gfx::TexId, u32, u32); 3],
    link_connected_face: (slot_gfx::TexId, u32, u32),
    link_hints: [(slot_gfx::TexId, u32); 3],
    link_switch_hints: [(slot_gfx::TexId, u32); 4],
    link_connected_hints: [(slot_gfx::TexId, u32); 2],
    link_failed_hints: [(slot_gfx::TexId, u32); 1],
    board_texture: Option<slot_gfx::TexId>,
    chip_texture: slot_gfx::TexId,
    picker: Option<CorePicker>,
    socket_textures: [slot_gfx::TexId; 2],
    chip_textures: [slot_gfx::TexId; 2],
    chip_blank: slot_gfx::TexId,
    chip_shadow: slot_gfx::TexId,
    core_legend_faces: [(slot_gfx::TexId, u32); 3],
    a_down_at: Option<Instant>,
    home_launch_held: bool,
    fresh_launch: bool,
    mode_down_at: Option<Instant>,
    eject_sound_armed: bool,
    mode_last_tap: Option<Instant>,
    polaroids: Option<Polaroids>,
    photo_textures: Vec<slot_gfx::TexId>,
    polaroid_title_texture: Option<slot_gfx::TexId>,
    hud: Hud,
    clock_text: String,
    clock_texture: Option<slot_gfx::TexId>,
    clock_face: Printed,
    battery_percent: Option<u8>,
    battery_texture: Option<slot_gfx::TexId>,
    battery_face: Printed,
    battery: Option<Battery>,
    bolt_texture: slot_gfx::TexId,
    bolt_light_texture: slot_gfx::TexId,
    letter_texture: Option<slot_gfx::TexId>,
    letter_face: Printed,
    letter_shown_at: Option<Instant>,
    // Unmodified upstream system names: 200ms in, 1200ms hold, 800ms out.
    platform_faces: [Printed; 3],
    recents_platform_face: Printed,
    favorites_platform_face: Printed,
    focus_seen: Option<(usize, String)>,
    focus_dirty_since: Option<Instant>,
    // Upstream v1.7 menu key-repeat belongs to the UI clock, never libretro.
    menu_held: Option<(i32,u64,usize)>,
    menu_repeat_dispatch: bool,
    platform_shown_at: Option<Instant>,
    selected_title_name: String,
    selected_title_face: Printed,
    selected_title_texture: Option<slot_gfx::TexId>,
}

/// The upstream wallpaper.deck expects Config/ to exist on an SD card.
/// Android's fresh app-private filesDir does not necessarily contain it yet.
fn pick_wallpaper_android(root: &Path) -> Option<PathBuf> {
    if let Err(error) = std::fs::create_dir_all(root.join("Config")) {
        eprintln!("slot-konkr: cannot initialize wallpaper deck: {error}");
    }
    crate::wallpaper::pick(root, 0)
}

/// Reuse Slot v1.7.0's exact wallpaper selector. Only Android app-private
/// Wallpapers/ files are eligible: never access ROMs or external SAF paths.
fn wallpaper_pixels(root: Option<&Path>, selection: &crate::settings::WallpaperPick)
    -> (Option<Vec<u8>>, bool)
{
    use crate::settings::WallpaperPick;
    let default = || slot_ui::default_background_native().map(|v|v.to_vec());
    let personal = match (selection, root) {
        (WallpaperPick::None, _) => return (None, false),
        (WallpaperPick::Default, _) => return (default(), false),
        (WallpaperPick::Blur, _) =>
            return (slot_ui::blur_background_native().map(|v|v.to_vec()), false),
        (WallpaperPick::File(name), Some(root)) => {
            if crate::wallpaper::list(root).contains(name) {
                Some(root.join("Wallpapers").join(name))
            } else { None }
        }
        (WallpaperPick::Random, Some(root)) => pick_wallpaper_android(root),
        _ => None,
    };
    if let Some(image) = personal.and_then(|p|slot_ui::wallpaper_face_native(&p)) {
        (Some(image), true)
    } else {
        // Official Slot falls back to the bundled default for an empty random
        // deck or a deleted file. Never silently mutate the saved selection.
        (default(), false)
    }
}

impl Engine {
    fn new(library_version: u64, library: Option<&[RomEntry]>) -> Result<Self, String> {
        let size = (960, 640);
        let mut gpu = Compositor::new(&AndroidSurface { size }).map_err(|e| e.to_string())?;
        let settings = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .as_ref().map(|(root, _)| Settings::load(root))
            .unwrap_or_else(Settings::first_install_defaults);
        // Upstream Slot delegates colour correction to libretro core options.
        // Disable the dev13 approximate GPU grade to avoid double correction.
        gpu.set_colour_correction(false);
        FF_AUDIO_ENABLED.store(settings.ff_sound, Ordering::Release);
        let visuals_root = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .as_ref().map(|(root, _)| root.clone());
        if let Some(root) = visuals_root.as_deref() {
            slot_ui::set_theme(slot_store::Theme::read(root));
        }
        let custom_wallpaper_present = visuals_root.as_deref()
            .is_some_and(|root| root.join("Wallpapers/user.png").is_file());
        let (wallpaper_pixels, wallpaper_personal) =
            wallpaper_pixels(visuals_root.as_deref(), &settings.wallpaper);
        let wallpaper_texture = wallpaper_pixels.map(|rgba|
            gpu.create_texture(slot_ui::NATIVE_W, slot_ui::NATIVE_H, &rgba));
        // Preserve existing Config/theme.txt custom colours as an opt-out.
        let custom_colours = visuals_root.as_deref()
            .is_some_and(|root| root.join("Config/theme.txt").exists());
        let mut make_slot = |light: bool| {
            let (back, front) = if light {
                (slot_ui::bitmaps::slot_back_light(), slot_ui::bitmaps::slot_front_light())
            } else {
                (slot_ui::bitmaps::slot_back(), slot_ui::bitmaps::slot_front())
            };
            let (back, w, h) = slot_ui::native_rgba(
                back, slot_ui::bitmaps::SLOT_W, slot_ui::bitmaps::SLOT_H);
            let rear = gpu.create_texture(w, h, &back);
            let (front, w, h) = slot_ui::native_rgba(
                front, slot_ui::bitmaps::SLOT_W, slot_ui::bitmaps::SLOT_H);
            let front = gpu.create_texture(w, h, &front);
            (rear, front)
        };
        // Both official themes are cached once. Toggling doesn't allocate
        // textures on the GL thread or shift the original Slot animation.
        let slot_dark_faces=make_slot(false);
        let slot_light_faces=make_slot(true);
        let slot_faces = if custom_colours { None }
            else if settings.appearance == crate::settings::Appearance::Light {
                Some(slot_light_faces)
            } else { Some(slot_dark_faces) };
        // Official 720x480 GB/GBC screen bezels, sampled once for KONKR.
        let (gb, w, h)=slot_ui::native_rgba(slot_ui::bitmaps::bezel_gb(),720,480);
        let gb_bezel_tex=gpu.create_texture(w,h,&gb);
        let (gbc,w,h)=slot_ui::native_rgba(slot_ui::bitmaps::bezel_gbc(),720,480);
        let gbc_bezel_tex=gpu.create_texture(w,h,&gbc);

        let mut shelves = Vec::new();

        // First-run empty shelf: never synthesize fake demo cartridges.
        let grouped = library.map(carts_by_platform)
            .unwrap_or_else(|| std::array::from_fn(|_| Vec::new()));
        for mut carts in grouped {
            if let Some(root) = visuals_root.as_deref() {
                for cart in &mut carts {
                    let uri = cart.rom.to_string_lossy();
                    let filename = format!("{}.png", core_selection::key(&uri));
                    // Only app-private labels are used; never rewrite ROMs.
                    cart.label = Some(root.join("Labels").join(filename));
                }
            }
            shelves.push(Shelf::new(carts));
        }
        // Fourth rack mirrors Slot's ten most recently launched ROMs.
        // Do not modify the three platform shelves or their ROM roots.
        let recent_ids=visuals_root.as_deref().map(crate::recents::load)
            .unwrap_or_default();
        let recent_carts=crate::recents::carts(&recent_ids,
            &[&shelves[0].carts, &shelves[1].carts, &shelves[2].carts]);
        shelves.push(Shelf::new(if settings.show_recents {
            recent_carts
        } else { Vec::new() }));
        // The v1.7 Favorites rack contains real GB/GBC/GBA carts in name order.
        // URI identity keeps identical ROM filenames in distinct folders separate.
        let favorite_ids=visuals_root.as_deref().map(crate::favorites::load)
            .unwrap_or_default();
        let favorite_carts=crate::favorites::carts(&favorite_ids,
            &[&shelves[0].carts,&shelves[1].carts,&shelves[2].carts]);
        shelves.push(Shelf::new(favorite_carts));

        // Original Slot names, not abbreviations: Game Boy Advance,
        // Game Boy, Game Boy Color. Keep the existing 3x antialiased 16px
        // font, centered placement and official 200/1200/800ms fade.
        // Cache textures once rather than rebuilding on every L1/R1.
        let platform_faces = Platform::ALL.map(|platform| {
            let face = platform_name_face(platform.name());
            let tex = gpu.create_texture(face.w, face.h, &face.rgba);
            Printed::new(tex, face.w)
        });

        let recent_label = platform_name_face("RECENTS");
        let recents_platform_face=Printed::new(
            gpu.create_texture(recent_label.w,recent_label.h,&recent_label.rgba),
            recent_label.w);
        let favorites_label=platform_name_face("FAVORITES");
        let favorites_platform_face=Printed::new(
            gpu.create_texture(favorites_label.w,favorites_label.h,&favorites_label.rgba),
            favorites_label.w);

        let shadow = cart_shadow();
        let gba = gpu.create_texture(shadow.w, shadow.h, &shadow.rgba);
        let shadow = gb_cart_shadow(GbShell::Notched);
        let gb = gpu.create_texture(shadow.w, shadow.h, &shadow.rgba);
        let shadow = gb_cart_shadow(GbShell::Rounded);
        let gbc = gpu.create_texture(shadow.w, shadow.h, &shadow.rgba);
        // The exact original Slot 9x9 pixel star, bright yellow with dark rim.
        let star=slot_ui::star_face(2, [0xf6,0xc3,0x2c]);
        let star_tex=gpu.create_texture(star.w,star.h,&star.rgba);
        for shelf in &mut shelves {
            shelf.set_shadow(gba);
            shelf.set_gb_shadow(GbShell::Notched, gb);
            shelf.set_gb_shadow(GbShell::Rounded, gbc);
            shelf.set_star(star_tex,star.w,star.h);
            shelf.mark_starred(|cart|favorite_ids.iter()
                .any(|uri|uri==cart.rom.to_string_lossy().as_ref()));
        }

        // Exactly the four Slot upstream SVG parts, original link_screen
        // positions/easing, pre-rasterised once on the GL thread.
        let art = slot_ui::link_art();
        let mut sprite = |face: slot_ui::CartFace| Sprite {
            tex: gpu.create_texture(face.w, face.h, &face.rgba),
            w: face.w, h: face.h,
        };
        let link_art = LinkSprites {
            port: sprite(art.port),
            plug_host: sprite(art.plug_host),
            plug_join: sprite(art.plug_join),
            adapter: sprite(art.adapter),
            arcs_right: art.arcs_right.map(&mut sprite),
            arcs_left: art.arcs_left.map(&mut sprite),
            clicks: sprite(art.clicks),
            arrow_left: sprite(art.arrow_left),
            arrow_right: sprite(art.arrow_right),
        };
        let menu_textures = MENU_TEXT.iter().map(|label| {
            let face = quick_value_face(label, true);
            let tex = gpu.create_texture(face.w, face.h, &face.rgba);
            (tex, face.w, face.h)
        }).collect();
        let menu_hints = [("A", "Select"), ("B", "Back")].map(|(key, label)| {
            let face = hint_face(key, label);
            let tex = gpu.create_texture(face.w, face.h, &face.rgba);
            (tex, face.w)
        });
        // Authentic Slot raster/keycap for a clean library. Render it in the
        // GL scene rather than above it in an Android TextView.
        let first_run_hint = {
            let face = onboarding_instruction_face(
                "PRESS", "START", ", THEN SELECT LIBRARY TO ADD GAMES");
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
        };
        let shelf_no_games_hint = {
            let face = onboarding_instruction_face(
                "PRESS", "START", "TO SELECT YOUR ROM FOLDER");
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
        };
        let onboarding_note = {
            let face = onboarding_note_face("(UNZIPPED ROMS ONLY)");
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
        };
        let shelf_status_faces = SHELF_STATUS_TEXT.map(|label| {
            let face = quick_value_face(label, true);
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w, face.h)
        });
        let shelf_status_start = {
            let face = hint_face("START", "Open Library");
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
        };
        // Original Slot keycap typography also describes the actual in-game
        // shortcuts, so users need not guess physical controller buttons.
        let game_menu_hints = [
            ("B", "Back"), ("SEL+R1", "Save"), ("SEL+L1", "Load"),
        ].map(|(key, label)| {
            let face = hint_face(key, label);
            let tex = gpu.create_texture(face.w, face.h, &face.rgba);
            (tex, face.w)
        });
        let make_link_text = |gpu: &mut Compositor, label: &str| {
            // Identical font, 30px scale, ink, centering and side padding
            // to original slot/src/frontend.rs menu_faces(), not our
            // Android Settings quick_value_face fallback.
            let face = menu_face(label);
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w, face.h)
        };
        let link_role_faces = LinkRow::ALL
            .map(|role| make_link_text(&mut gpu, role.text()));
        let link_step_faces = LinkStep::ALL
            .map(|step| make_link_text(&mut gpu, step.line()));
        let link_fail_faces = LinkFail::SHOWN
            .map(|failure| make_link_text(&mut gpu, failure.line()));
        let link_connected_face = make_link_text(&mut gpu, "Linked");
        let link_hints = [
            hint_face("B", "Cancel"),
            arrows_hint_face("Swap"),
            hint_face("A", "Link"),
        ].map(|face| {
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
        });
        // State-specific original Slot legends; never advertise an active
        // Connect button when already linked or after a failed handshake.
        let link_connected_hints = [("B", "Back"), ("A", "End Link")]
            .map(|(key, label)| {
                let face = hint_face(key, label);
                (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
            });
        let link_failed_hints = [hint_face("A", "OK")].map(|face| {
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
        });
        // Upstream has SELECT Mode in the picker when Cable/RFU is truly
        // switchable. Reuse exactly its hint and arrow primitives.
        let link_switch_hints = [
            hint_face("B", "Cancel"),
            hint_face("SELECT", "Mode"),
            arrows_hint_face("Swap"),
            hint_face("A", "Link"),
        ].map(|face| {
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w)
        });
        let setting_values = ["2×", "3×", "4×", "6×", "ON", "OFF",
                              "LCD3x", "Grid", "Dot", "Simpletex",
                              "STANDARD", "RESPONSIVE", "1 FRAME", "2 FRAMES",
                              "DISPLAY SYNC", "TURBO", "A/B",
                              "None", "Random", "Default", "DEFAULT BLUR",
                              "Dark", "Light", "Low", "Medium", "High", "Custom",
                               "LCD Grid V2", "Grid V2 + GBA Color", "zfast LCD",
                               "LCD1x", "Simpletex + GBA", "AGB-001",
                               "AGS-001", "Sharp Bilinear"]
            .into_iter().map(|value| {
                let face = quick_value_face(value, true);
                let tex = gpu.create_texture(face.w, face.h, &face.rgba);
                (tex, face.w, face.h)
            }).collect();
        let wallpaper_name_face = match &settings.wallpaper {
            crate::settings::WallpaperPick::File(_) => {
                let name = settings.wallpaper.label();
                let short: String = name.chars().take(18).collect();
                let face = quick_value_face(&short, true);
                Some((gpu.create_texture(face.w, face.h, &face.rgba), face.w, face.h))
            }
            _ => None,
        };
        let emulation_core_labels = Core::ALL.map(|core| {
            let face = quick_value_face(core.text(), true);
            (gpu.create_texture(face.w, face.h, &face.rgba), face.w, face.h)
        });
        let setting_carets = [false, true].map(|right| {
            let face = quick_caret_face(right);
            let tex = gpu.create_texture(face.w, face.h, &face.rgba);
            (tex, face.w, face.h)
        });
        let settings_hints = [
            hint_face("B", "Back"), arrows_hint_face("Change")
        ].map(|face| {
            let tex = gpu.create_texture(face.w, face.h, &face.rgba);
            (tex, face.w)
        });
        // Rasterize the detailed barcode About label only when opened.
        // Creating SVG / fonts during every cold boot needlessly delays shelf.
        let about_texture = None;
        let chip = chip_face(Some(Core::Mgba));
        let chip_texture = gpu.create_texture(chip.w, chip.h, &chip.rgba);
        let socket_textures = Core::ALL.map(|core| {
            let face = socket_face(core);
            gpu.create_texture(face.w, face.h, &face.rgba)
        });
        let chip_textures = Core::ALL.map(|core| {
            if core == Core::Mgba { return chip_texture; }
            let face = chip_face(Some(core));
            gpu.create_texture(face.w, face.h, &face.rgba)
        });
        let blank = chip_face(None);
        let chip_blank = gpu.create_texture(blank.w, blank.h, &blank.rgba);
        let shadow = chip_shadow_face();
        let chip_shadow = gpu.create_texture(shadow.w, shadow.h, &shadow.rgba);
        // The actual Slot legend is a set of three compact keycaps, not
        // oversized menu labels. These are the original Slot UI generators.
        let core_legend_faces = [
            hint_face("B", "Cancel"),
            arrows_hint_face("Swap"),
            hint_face("A", "Choose"),
        ].map(|face| {
            let texture = gpu.create_texture(face.w, face.h, &face.rgba);
            (texture, face.w)
        });

        // Use the original Slot toast faces and HUD, not Android text banners.
        let mut hud = Hud::new();
        let toasts = Toast::all().into_iter().map(|toast| {
            let face = toast_face(toast);
            gpu.create_texture(face.w, face.h, &face.rgba)
        }).collect();
        hud.set_toasts(toasts);
        // The upstream glyph subset is already packaged. Enable its exact
        // fast-forward badge and rewind progress icon in the Android HUD.
        let icons = Icon::ALL.into_iter().map(|icon| {
            let face = icon_face(icon, slot_ui::HUD_ICON_PX, HUD_INK);
            gpu.create_texture(face.w, face.h, &face.rgba)
        }).collect();
        hud.set_icons(icons);
        // Exact upstream slot. Link badge glyphs, colour and 24px halo.
        // The HUD already places these in the top-right of the game image,
        // but Android previously never populated its link texture atlas.
        let link_faces = LinkBadge::FACES.into_iter().map(|badge| {
            let glyph = badge.badge().expect("only visible LinkBadge variants");
            let colour = badge.colour().expect("visible LinkBadge has a colour");
            let face = badge_face(glyph, slot_ui::HUD_ICON_PX, colour);
            gpu.create_texture(face.w, face.h, &face.rgba)
        }).collect();
        hud.set_link_faces(link_faces);
        let bolt = icon_face(Icon::Charging, BOLT_PX, slot_ui::footer_ink(false));
        let bolt_texture = gpu.create_texture(bolt.w, bolt.h, &bolt.rgba);
        let bolt = icon_face(Icon::Charging, BOLT_PX, slot_ui::footer_ink(true));
        let bolt_light_texture = gpu.create_texture(bolt.w, bolt.h, &bolt.rgba);

        // On a GBA-only library do not begin on an empty GB or GBC shelf.
        let saved_focus=visuals_root.as_deref().and_then(crate::selection::load);
        let active=saved_focus.as_ref().and_then(|focus|restore_saved_focus(&mut shelves,focus))
            .unwrap_or_else(||shelves.iter().position(|s|!s.carts.is_empty()).unwrap_or(0));
        let now = Instant::now();
        let has_multiple_systems = shelves.iter().filter(|s| !s.carts.is_empty()).count() > 1;
        Ok(Self {
            gpu,
            size,
            shelves,
            active,
            inserted: false,
            progress: 0.0,
            refusal: None,
            insert_started: None,
            screen_power: 0.0,
            exiting_screen: false,
            eject_hold_remaining: 0.0,
            select_down_at: None,
            select_home_consumed: false,
            select_chorded: false,
            select_forwarded: false,
            select_tap_frames: 0,
            chord_releases: 0,
            born: now,
            last: now,
            library_version,
            texture_cache: VecDeque::new(),
            last_cart_navigation: now,
            cart_render: CartRender::start(visuals_root.as_deref())?,
            clear_material: None,
            game: None,
            pending_game: None,
            link_identity: None,
            link_role: LinkRow::Host,
            link_mode: LinkKind::Cable,
            link_mode_override: None,
            link_phase: LinkMenu::Pick(LinkRow::Host),
            link_entry: LinkEntry::GameMenu,
            link_worked_at: 0,
            link_starter: None,
            link_art,
            ra_service: None,
            ra_signing_out: false,
            buttons: 0,
            ff_held: false,
            ff_latched: false,
            ff_last_release: None,
            rewind_held: false,
            rewind_accum: 0.0,
            awaiting_game: false,
            requested_uri: None,
            prepared_rom: None,
            game_accum: 0.0,
            perf_last: now,
            perf_rendered: 0,
            perf_emulated: 0,
            perf_catchup: 0,
            external_pending: false,
            overlay: ShelfOverlay::None,
            settings,
            game_emulation: None,
            emulation_core_labels,
            video_mode: VideoMode::Actual,
            wallpaper_texture,
            wallpaper_personal,
            wallpaper_name_face,
            slot_faces,
            slot_dark_faces,
            slot_light_faces,
            gb_bezel_tex,
            gbc_bezel_tex,
            custom_wallpaper_present,
            setting_values,
            setting_carets,
            settings_hints,
            about_texture,
            menu_seen: 0,
            menu_opened: now,
            menu_cursor_y: 0.0,
            menu_textures,
            menu_hints,
            first_run_hint,
            onboarding_note,
            shelf_no_games_hint,
            shelf_status: 0,
            shelf_status_faces,
            shelf_status_start,
            game_menu_hints,
            link_role_faces,
            link_step_faces,
            link_fail_faces,
            link_connected_face,
            link_hints,
            link_switch_hints,
            link_connected_hints,
            link_failed_hints,
            board_texture: None,
            chip_texture,
            picker: None,
            socket_textures,
            chip_textures,
            chip_blank,
            chip_shadow,
            core_legend_faces,
            a_down_at: None,
            home_launch_held: false,
            fresh_launch: false,
            mode_down_at: None,
            eject_sound_armed: false,
            mode_last_tap: None,
            polaroids: None,
            photo_textures: Vec::new(),
            polaroid_title_texture: None,
            hud,
            clock_text: String::new(),
            clock_texture: None,
            clock_face: Printed::default(),
            battery_percent: None,
            battery_texture: None,
            battery_face: Printed::default(),
            battery: None,
            bolt_texture,
            bolt_light_texture,
            letter_texture: None,
            letter_face: Printed::default(),
            letter_shown_at: None,
            platform_faces,
            recents_platform_face,
            favorites_platform_face,
            focus_seen: None,
            focus_dirty_since: None,
            menu_held: None,
            menu_repeat_dispatch: false,
            platform_shown_at: has_multiple_systems.then_some(now),
            selected_title_name: String::new(),
            selected_title_face: Printed::default(),
            selected_title_texture: None,
        })
    }

    fn shelf_footer_ink(&self) -> [u8; 3] {
        slot_ui::footer_ink(
            self.settings.appearance == crate::settings::Appearance::Light)
    }

    fn recolour_footer_text(&mut self) {
        let ink = self.shelf_footer_ink();
        if let Some(tex) = self.clock_texture {
            let face = slot_ui::word_ink(&self.clock_text, ink);
            self.gpu.update_texture(tex, face.w, face.h, &face.rgba);
            self.clock_face = Printed::new(tex, face.w);
        }
        if let (Some(tex), Some(percent)) = (self.battery_texture, self.battery_percent) {
            let face = slot_ui::word_ink(&format!("{percent}%"), ink);
            self.gpu.update_texture(tex, face.w, face.h, &face.rgba);
            self.battery_face = Printed::new(tex, face.w);
        }
    }

    fn refresh_system_status(&mut self) {
        let status = SYSTEM_STATUS.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if status.clock != self.clock_text {
            self.clock_text = status.clock;
            if !self.clock_text.is_empty() {
                let face = slot_ui::word_ink(&self.clock_text, self.shelf_footer_ink());
                let tex = match self.clock_texture {
                    Some(tex) => { self.gpu.update_texture(tex, face.w, face.h, &face.rgba); tex }
                    None => { let tex = self.gpu.create_texture(face.w, face.h, &face.rgba);
                        self.clock_texture = Some(tex); tex }
                };
                self.clock_face = Printed::new(tex, face.w);
            }
        }
        self.battery = status.battery;
        let percent = self.battery.map(|b| b.percent);
        if percent != self.battery_percent {
            self.battery_percent = percent;
            // Just as with original Slot, the About label reflects the live
            // battery without rebuilding it on every animation frame.
            if let Some(tex) = self.about_texture {
                let sticker = sticker_face_konkr(&StickerFields {
                    battery: percent, serial: "0000130", dirty_digit: '0',
                });
                self.gpu.update_texture(tex, sticker.w, sticker.h, &sticker.rgba);
            }
            if let Some(percent) = percent {
                let face = slot_ui::word_ink(&format!("{percent}%"), self.shelf_footer_ink());
                let tex = match self.battery_texture {
                    Some(tex) => { self.gpu.update_texture(tex, face.w, face.h, &face.rgba); tex }
                    None => { let tex = self.gpu.create_texture(face.w, face.h, &face.rgba);
                        self.battery_texture = Some(tex); tex }
                };
                self.battery_face = Printed::new(tex, face.w);
            } else { self.battery_face = Printed::default(); }
        }
    }

    fn reset_time_controls(&mut self) {
        self.ff_held = false;
        self.ff_latched = false;
        self.ff_last_release = None;
        self.rewind_held = false;
        self.rewind_accum = 0.0;
        self.game_accum = 0.0;
        self.select_down_at = None;
        self.select_home_consumed = false;
        self.select_chorded = false;
        self.select_forwarded = false;
        self.select_tap_frames = 0;
        self.chord_releases = 0;
        self.hud.set_ff(FfState::Off);
        self.hud.release_rewind();
        AUDIO_SPEED_PERMILLE.store(1000, Ordering::Release);
        RUMBLE_STRENGTH.store(0, Ordering::Release);
    }


    // Upstream slot-input gestures: SELECT chords have priority over game input.
    // Distinct release bits avoid leaking A/B/shoulder releases into libretro.
    fn handle_select_chord(&mut self, code: i32, pressed: bool) -> bool {
        let bit = select_chord_bit(code);
        if !pressed && bit != 0 && self.chord_releases & bit != 0 {
            self.chord_releases &= !bit;
            return true;
        }
        if !pressed || bit == 0 || self.select_down_at.is_none() { return false; }
        self.chord_releases |= bit;
        self.select_chorded = true;
        self.select_tap_frames = 0;
        self.select_forwarded = false;
        self.buttons &= !ButtonMask::SELECT;
        let time = self.born.elapsed().as_millis() as u64;
        match code {
            96 | 97 => {
                let key = if self.current_cart_platform() == Platform::Gba { 0 } else { 1 };
                if self.game.is_some() {
                    self.change_game_screen_or_gameplay(key,code == 96);
                } else { self.change_setting(key,code == 96); }
                let shader = self.game_shader();
                self.gpu.set_screen_effect(shader.effect());
                self.hud.toast(match shader {
                    crate::settings::Shader::Off => Toast::ShaderOff,
                    crate::settings::Shader::Lcd3x => Toast::ShaderLcd3x,
                    crate::settings::Shader::Grid => Toast::ShaderGrid,
                    crate::settings::Shader::Dot => Toast::ShaderDot,
                    crate::settings::Shader::Simpletex => Toast::ShaderSimpletex,
                     crate::settings::Shader::LcdGridV2 => Toast::ShaderLcdGridV2,
                     crate::settings::Shader::LcdGridV2Gba => Toast::ShaderLcdGridV2Gba,
                     crate::settings::Shader::ZfastLcd => Toast::ShaderZfastLcd,
                     crate::settings::Shader::Lcd1x => Toast::ShaderLcd1x,
                     crate::settings::Shader::SimpletexGba => Toast::ShaderSimpletexGba,
                     crate::settings::Shader::Agb001 => Toast::ShaderAgb001,
                     crate::settings::Shader::Ags001 => Toast::ShaderAgs001,
                     crate::settings::Shader::SharpBilinear => Toast::ShaderSharpBilinear,
                }, time);
            }
            100 => {
                if self.game.is_some() {
                    self.change_game_screen_or_gameplay(2,true);
                } else { self.change_setting(2,true); }
                self.hud.toast(if self.game_settings().colour_correction {
                    Toast::ColourOn
                } else { Toast::ColourOff }, time);
            }
            102 => {
                if let Some(game) = self.game.as_mut() {
                    match game.load_latest() {
                        Ok(()) => self.hud.toast(Toast::StateLoaded, time),
                        Err(err) => set_message(format!("Cannot load state: {err}")),
                    }
                }
            }
            103 => {
                if let Some(game) = self.game.as_mut() {
                    match game.save_manual() {
                        Ok(stamp) => {
                            self.hud.toast(Toast::StateSaved, time);
                            if let Some(uri) = self.requested_uri.as_deref() {
                                let paths = PATHS.lock().unwrap_or_else(|e| e.into_inner()).clone();
                                let core = paths.as_ref().map(|(storage, _)| {
                                    core_selection::selected(storage, uri).as_str()
                                }).unwrap_or("mgba");
                                let event = serde_json::json!({
                                    "uri": uri, "core": core, "manual_stamp": stamp
                                }).to_string();
                                SAVE_SYNC.lock().unwrap_or_else(|e| e.into_inner())
                                    .push_back(event);
                            }
                        }
                        Err(err) => set_message(format!("Cannot export state: {err}")),
                    }
                }
            }
            104 | 105 => {
                // Also valid for GB-compatible .gbc ROMs (CGB flag bit 7 unset).
                if self.game_settings().gb_palettes
                    && self.game.as_ref().is_some_and(|g| g.can_change_gb_palette())
                {
                    let index = if code == 105 {
                        (self.settings.gb_palette + 1) % 48
                    } else { (self.settings.gb_palette + 47) % 48 };
                    self.settings.gb_palette = index;
                    if let Some(game) = self.game.as_mut() {
                        game.set_gb_palette(self.settings.palette_name());
                    }
                    let palette = slot_store::GbPalette::all().nth(index as usize)
                        .unwrap_or(slot_store::GbPalette::DEFAULT);
                    self.hud.toast(Toast::Palette(palette), time);
                    self.persist_settings();
                }
            }
            _ => {}
        }
        true
    }

    fn link_millis(&self) -> u64 {
        self.born.elapsed().as_millis() as u64
    }

    fn cancel_link_discovery(&mut self) {
        if let Some(starter) = self.link_starter.take() { starter.cancel(); }
        LINK_NEEDS_MULTICAST.store(false, Ordering::Release);
    }

    fn enter_multiplayer_menu(&mut self) {
        if self.game.is_none() { return; }
        if self.game.as_ref().is_some_and(GameSession::requires_relaunch) {
            set_message("Eject and relaunch the cartridge before linking again".into());
            return;
        }
        if self.link_identity.is_none() {
            set_message("Multiplayer currently requires a valid GBA ROM".into());
            return;
        }
        let game = self.game.as_ref().expect("checked above");
        let identity = self.link_identity.as_ref().expect("checked above");
        if game.core_kind() == Core::Mgba {
            // Slot original lockstep uses Cable only. The core is reopened
            // in dual-GBA mode after the network connection succeeds.
            self.link_mode = LinkKind::Cable;
        } else {
            if !game.wireless_capable() {
                set_message("This gpSP build did not register its GBA Link interface".into());
                return;
            }
            if !link_kind::link_carried(&identity.code, &identity.title) {
                crate::game::log_launch_timing(&format!(
                    "Multiplayer unsupported protocol: game_code={}, title={}, clean_header={}",
                    identity.code, identity.title, identity.clean_header
                ));
                set_message("This GBA game's link protocol is not yet verified in gpSP. Try Pokemon FireRed or Emerald.".into());
                return;
            }
            let automatic = link_kind::link_kind(&identity.code, &identity.title,
                identity.clean_header);
            self.link_mode = self.link_mode_override.unwrap_or_else(||
                link_kind::preferred_mode(&identity.code, &identity.title,
                    identity.clean_header));
            if !link_kind::gpsp_runtime_mode_supported(&identity.code, self.link_mode) {
                set_message("This gpSP core cannot emulate this cartridge's GBA Cable protocol. Try mGBA Cable with identical ROMs.".into());
                return;
            }
        }
        self.cancel_link_discovery();
        self.link_role = LinkRow::Host;
        self.link_phase = LinkMenu::Pick(self.link_role);
        self.game_accum = 0.0;
        self.buttons = 0;
        self.overlay = ShelfOverlay::Multiplayer;
    }

    /// Upstream SELECT Mode is only offered if its two values produce
    /// genuinely different gpSP serial hardware configurations. mGBA is
    /// intentionally Cable-only with this Android dual-GBA Lab backend.
    fn link_mode_switchable(&self) -> bool {
        let Some(identity) = self.link_identity.as_ref() else { return false };
        let Some(game) = self.game.as_ref() else { return false };
        link_mode_can_switch(game.core_kind(), self.link_mode,
            &identity.code, &identity.title, identity.clean_header)
    }

    /// Original Slot/fork reload the gpSP core when SELECT changes the
    /// serial hardware. Never begin a connection with a cosmetic mode that
    /// differs from the live libretro serial implementation.
    fn ensure_gpsp_link_serial(&mut self) -> Result<(), String> {
        let game = self.game.as_ref().ok_or("No running GBA game")?;
        if game.core_kind() != Core::Gpsp { return Ok(()); }
        let identity = self.link_identity.as_ref().ok_or("No valid GBA ROM identity")?;
        let automatic = link_kind::link_kind(&identity.code, &identity.title,
            identity.clean_header);
        let wanted = link_kind::serial_option(self.link_mode, automatic,
            &identity.code, &identity.title);
        let previous = game.loaded_serial();
        if wanted == previous { return Ok(()); }

        let (storage, library) = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .clone().ok_or("Slot storage not mounted")?;
        let (rom, stem) = (game.rom_path().to_path_buf(), game.rom_stem().to_owned());
        let core_file = library.join("libgpsp_libretro.so");
        // An atomic private state+SRAM+RTC snapshot is required BEFORE the
        // existing core is dropped. The one-shot marker makes the reloaded
        // core prefer this state over a stale RetroArch auto-save.
        self.game.as_mut().ok_or("Game disappeared before serial switch")?
            .preserve_solo_for_link()?;
        drop(self.game.take());
        let resume = |serial| GameSession::open_gpsp_serial(
            &rom, &core_file, &storage, &stem, self.settings.colour_correction,
            self.ra_service.as_ref(), serial);
        let replacement = match resume(wanted) {
            Ok(game) => game,
            Err(error) => {
                // Reopen the last known working mode if the requested one
                // fails, retaining the pre-switch state and battery data.
                match resume(previous) {
                    Ok(original) => {
                        SAMPLE_RATE.store(original.sample_rate, Ordering::Release);
                        self.game = Some(original);
                        return Err(format!("Cannot activate gpSP {wanted}: {error}; previous mode restored"));
                    }
                    Err(recovery) => {
                        SAMPLE_RATE.store(0, Ordering::Release);
                        PLAYING.store(false, Ordering::Release);
                        self.handle(Input::Exit);
                        return Err(format!("gpSP Link mode failed: {error}; recovery requires cartridge relaunch ({recovery})"));
                    }
                }
            }
        };
        SAMPLE_RATE.store(replacement.sample_rate, Ordering::Release);
        AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
        RUMBLE_STRENGTH.store(0, Ordering::Release);
        self.game_accum = 0.0;
        self.buttons = 0;
        self.game = Some(replacement);
        crate::game::log_launch_timing(&format!(
            "gpSP serial reload: {previous} -> {wanted} (upstream Link hardware selection)"));
        Ok(())
    }

    fn request_multiplayer_network(&mut self) {
        if !matches!(self.overlay, ShelfOverlay::Multiplayer) { return; }
        if matches!(self.link_phase, LinkMenu::Working {..} | LinkMenu::Linked {..}) { return; }
        if let Err(error) = self.ensure_gpsp_link_serial() {
            if self.game.is_some() {
                self.fail_multiplayer(&error, LinkFail::Radio);
            } else {
                set_message(error);
            }
            return;
        }
        self.cancel_link_discovery();
        let now = self.link_millis();
        self.link_worked_at = now;
        self.link_phase = LinkMenu::Working {
            role: self.link_role, step: LinkStep::Radio, since: now,
        };
        LINK_NEEDS_MULTICAST.store(true, Ordering::Release);
        UI_ACTION.lock().unwrap_or_else(|e| e.into_inner()).push_back(16);
    }

    /// A Lab session must negotiate exact backend, mode, ROM revision,
    /// compiled core and BIOS before touching the current solo timeline.
    /// The worker hashes all files; the GL/render thread only passes paths.
    fn link_config(&self) -> Result<crate::multiplayer::LinkConfig, String> {
        let session = self.game.as_ref().ok_or("No running game")?;
        let identity = self.link_identity.as_ref().ok_or("No GBA Link identity")?;
        let (storage, library) = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .clone().ok_or("Slot storage not mounted")?;
        let kind = session.core_kind();
        let backend = match kind {
            Core::Mgba => crate::multiplayer::Backend::MgbaSlotCable,
            Core::Gpsp => crate::multiplayer::Backend::GpspNetpacket,
        };
        Ok(crate::multiplayer::LinkConfig {
            game: identity.code.clone(),
            rom: session.rom_path().to_path_buf(),
            core_file: library.join(format!("lib{}_libretro.so", kind.as_str())),
            bios_file: storage.join("BIOS/gba_bios.bin"),
            backend,
            mode: self.link_mode,
            trusted_dir: storage.join("TrustedLinkDevices"),
        })
    }

    fn fail_multiplayer(&mut self, reason: &str, failure: LinkFail) {
        self.cancel_link_discovery();
        // No link has been established: never display a false connected icon.
        self.hud.set_link(LinkBadge::Off);
        self.link_phase = LinkMenu::Failed {
            role: self.link_role, fail: failure,
            worked: self.link_worked_at, since: self.link_millis(),
        };
        set_message(format!("Multiplayer: {reason}"));
    }

    fn begin_link_session(&mut self, role: LinkRole,
                          wire: crate::link_net::TcpLink,
                          peer_rom: Option<PathBuf>) -> Result<(), String> {
        let game = self.game.as_ref().ok_or("No game active")?;
        if game.core_kind() == Core::Gpsp {
            if game.linked() || game.is_link_sandbox() {
                return Err("gpSP Link requires a fresh private session".into());
            }
            let (storage, library) = PATHS.lock().unwrap_or_else(|e| e.into_inner())
                .clone().ok_or("Slot storage is not mounted")?;
            let (rom, stem, serial) = (game.rom_path().to_path_buf(),
                game.rom_stem().to_owned(), game.loaded_serial());
            let core_file = library.join("libgpsp_libretro.so");
            // Close solo only AFTER its exact state/SRAM/RTC are durable.
            // A live gpSP core keeps the solo libretro GET_SAVE_DIRECTORY;
            // redirecting Slot's own save paths is insufficient isolation.
            let solo_snapshot = self.game.as_mut().ok_or("No game active")?
                .preserve_solo_for_link()?;
            drop(self.game.take());
            let mut private = match GameSession::open_gpsp_linked(
                &rom, &core_file, &storage, &stem,
                self.settings.colour_correction, role, &solo_snapshot, serial) {
                Ok(game) => game,
                Err(error) => {
                    let recovery = self.restore_gpsp_solo_after_link_failure(
                        &rom, &core_file, &storage, &stem, serial, &solo_snapshot);
                    return Err(link_boot_failure("Cannot initialize private gpSP Link",
                        &error, recovery));
                }
            };
            if let Err(error) = private.begin_multiplayer(role, self.link_mode, Box::new(wire)) {
                drop(private);
                let recovery = self.restore_gpsp_solo_after_link_failure(
                    &rom, &core_file, &storage, &stem, serial, &solo_snapshot);
                return Err(link_boot_failure("Cannot begin private gpSP Link",
                    &error, recovery));
            }
            SAMPLE_RATE.store(private.sample_rate, Ordering::Release);
            AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
            RUMBLE_STRENGTH.store(0, Ordering::Release);
            self.game_accum = 0.0;
            self.buttons = 0;
            self.game = Some(private);
            return Ok(());
        }
        if self.link_mode != LinkKind::Cable || game.linked() || game.is_link_sandbox() {
            return Err("mGBA Link requires a fresh Cable session".into());
        }
        let (storage, library) = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .clone().ok_or("Slot storage is not mounted".to_owned())?;
        let rom = game.rom_path().to_path_buf();
        let stem = game.rom_stem().to_owned();
        let core_file = library.join("libmgba_libretro.so");
        // Save the current solo state/ram/RTC atomically BEFORE replacing
        // the core. Failure stops link without losing the solo instance.
        let solo_snapshot = self.game.as_mut()
            .ok_or("No game active".to_owned())?
            .preserve_solo_for_link()?;
        // The exact private solo snapshot is passed in-memory; the linked
        // engine loads it as both GBA halves before Host/Join state exchange.
        // The existing solo core remains alive until its checkpoint is durable.
        drop(self.game.take());
        let mut linked = match GameSession::open_mgba_linked(&rom, &core_file,
            &storage, &stem, self.settings.colour_correction, role,
            &solo_snapshot, peer_rom.as_deref()) {
            Ok(session) => session,
            Err(error) => {
                let recovery = self.restore_solo_after_link_failure(
                    &rom, &core_file, &storage, &stem, &solo_snapshot);
                return Err(link_boot_failure("Cannot initialize mGBA Link",
                    &error, recovery));
            }
        };
        if let Err(error) = linked.begin_multiplayer(role, LinkKind::Cable, Box::new(wire)) {
            drop(linked);
            let recovery = self.restore_solo_after_link_failure(
                &rom, &core_file, &storage, &stem, &solo_snapshot);
            return Err(link_boot_failure("Cannot begin mGBA Link",
                &error, recovery));
        }
        SAMPLE_RATE.store(linked.sample_rate, Ordering::Release);
        AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.game_accum = 0.0;
        self.buttons = 0;
        self.game = Some(linked);
        Ok(())
    }

    /// An explicit unplug is a core transition, not a cartridge ejection.
    /// Capture the live LOCAL player's GBA, destroy its private Link core,
    /// verify a fresh non-Link emulator before modifying any solo save,
    /// and promote an immutable, conflict-checked battery/state generation.
    fn return_link_to_solo(&mut self) -> Result<(), String> {
        let linked = self.game.as_mut().ok_or("No Link game to unplug")?;
        let which = linked.core_kind();
        let serial = linked.loaded_serial();
        let rom = linked.rom_path().to_path_buf();
        let stem = linked.rom_stem().to_owned();
        let rejoin = linked.prepare_solo_rejoin()?;
        let (storage, library) = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .clone().ok_or("Slot storage not configured")?;
        // libretro cores cannot safely have two independent active instances
        // in this single process. Store the complete private snapshot before
        // dropping the Link instance, never write to solo until verified.
        linked.end_multiplayer();
        drop(self.game.take());
        let dylib = library.join(format!("lib{}_libretro.so", which.as_str()));
        let mut solo = match which {
            Core::Mgba => GameSession::open(&rom, &dylib, &storage,
                which, Platform::Gba, false, &stem, None,
                self.settings.colour_correction, self.ra_service.as_ref()),
            Core::Gpsp => GameSession::open_gpsp_solo_unresumed(
                &rom, &dylib, &storage, &stem,
                self.settings.colour_correction, self.ra_service.as_ref(), serial),
        }?;
        solo.restore_local_after_link(&rejoin)?;
        let backup = solo.commit_rejoined_solo(&rejoin)?;
        crate::game::log_launch_timing(&format!(
            "Link -> SOLO validated for {} in {}, immutable backup={}",
            which.text(), stem, backup.display()));
        SAMPLE_RATE.store(solo.sample_rate, Ordering::Release);
        AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
        RUMBLE_STRENGTH.store(0, Ordering::Release);
        self.game_accum = 0.0;
        self.buttons = 0;
        self.reset_time_controls();
        self.game = Some(solo);
        // Kotlin may now export current SOLO SRAM and the newly regenerated
        // compatible RetroArch auto-state. It must not export private Link
        // session files, nor mix FireRed with LeafGreen.
        if let Some(uri) = self.requested_uri.as_deref() {
            let event = serde_json::json!({
                "uri": uri, "core": which.as_str(),
            }).to_string();
            SAVE_SYNC.lock().unwrap_or_else(|e| e.into_inner()).push_back(event);
        }
        Ok(())
    }

    fn restore_gpsp_solo_after_link_failure(&mut self, rom: &Path,
                                             core_file: &Path, storage: &Path,
                                             stem: &str, serial: &'static str,
                                             snapshot: &[u8]) -> Result<(), String> {
        // A successful core open is NOT evidence of a successful state
        // resume: libretro can reject the saved state and start at the title.
        // Re-apply the in-memory checkpoint and fail closed if it is rejected.
        let result = (|| {
            let mut game = GameSession::open_gpsp_serial(rom, core_file, storage, stem,
                self.settings.colour_correction, self.ra_service.as_ref(), serial)?;
            game.restore_checkpoint_after_link_failure(snapshot)?;
            SAMPLE_RATE.store(game.sample_rate, Ordering::Release);
            self.game = Some(game);
            Ok(())
        })();
        if result.is_err() {
            SAMPLE_RATE.store(0, Ordering::Release);
            PLAYING.store(false, Ordering::Release);
        }
        result
    }

    fn restore_solo_after_link_failure(&mut self, rom: &Path,
                                       core_file: &Path, storage: &Path,
                                       stem: &str, snapshot: &[u8]) -> Result<(), String> {
        let result = (|| {
            let mut game = GameSession::open(rom, core_file, storage,
                Core::Mgba, Platform::Gba, true, stem, None,
                self.settings.colour_correction, self.ra_service.as_ref())?;
            game.restore_checkpoint_after_link_failure(snapshot)?;
            SAMPLE_RATE.store(game.sample_rate, Ordering::Release);
            self.game = Some(game);
            Ok(())
        })();
        if result.is_err() {
            SAMPLE_RATE.store(0, Ordering::Release);
            PLAYING.store(false, Ordering::Release);
        }
        result
    }

    fn poll_multiplayer(&mut self) {
        if !matches!(self.overlay, ShelfOverlay::Multiplayer) { return; }
        let event = self.link_starter.as_mut().and_then(LinkStarter::poll);
        if let Some(event) = event {
            match event {
                LinkProgress::Looking => {
                    self.link_phase = LinkMenu::Working {
                        role: self.link_role, step: LinkStep::Waiting,
                        since: self.link_worked_at,
                    };
                }
                LinkProgress::PeerRomConsent(label) => {
                    *LINK_PEER_LABEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(label);
                    // Kotlin displays a Slot-styled choice and returns the
                    // decision asynchronously; GL stays nonblocking.
                    UI_ACTION.lock().unwrap_or_else(|e| e.into_inner()).push_back(18);
                }
                LinkProgress::Ready(wire, peer_rom) => {
                    self.cancel_link_discovery();
                    let role = if self.link_role == LinkRow::Host {
                        LinkRole::Host
                    } else { LinkRole::Join };
                    let joined = self.begin_link_session(role, wire, peer_rom);
                    match joined {
                        Ok(()) => {
                            // Host purple / Join grey, permanently shown in
                            // the exact upstream top-right in-game HUD.
                            self.hud.set_link(link_badge_for(self.link_role, false));
                            self.link_phase = LinkMenu::Linked {
                                role: self.link_role,
                                worked: self.link_worked_at,
                                since: self.link_millis(), opened: false,
                            };
                        }
                        Err(reason) => self.fail_multiplayer(&reason, LinkFail::PeerVanished),
                    }
                }
                LinkProgress::Failed(reason) => {
                    let fail = match reason {
                        crate::multiplayer::Failure::NoNetwork => LinkFail::Radio,
                        crate::multiplayer::Failure::NobodyCame => LinkFail::NobodyCame,
                        crate::multiplayer::Failure::IncompatiblePeer => LinkFail::PeerVanished,
                        crate::multiplayer::Failure::Cancelled => LinkFail::Cancelled,
                        crate::multiplayer::Failure::PeerVanished => LinkFail::PeerVanished,
                    };
                    let details = if reason == crate::multiplayer::Failure::IncompatiblePeer {
                        "Peer refused Link: check compatible cartridges, same build, BIOS and Cable/RFU mode (mGBA KMLB/4, gpSP KMLB/3)."
                    } else {
                        match reason {
                            crate::multiplayer::Failure::NobodyCame => "No compatible Host found. Ensure Host is waiting on the same LAN.",
                            crate::multiplayer::Failure::NoNetwork => "A usable Wi-Fi or emulator bridge network is required.",
                            crate::multiplayer::Failure::Cancelled => "Connection cancelled.",
                            _ => "Multiplayer peer disappeared or connection failed.",
                        }
                    };
                    self.fail_multiplayer(details, fail);
                }
            }
        }
        if link_menu_auto_closes(self.link_phase, self.link_millis()) {
            self.overlay = ShelfOverlay::None;
            self.game_accum = 0.0;
            self.buttons = 0;
        }
        if link_unplug_complete(self.link_phase, self.link_millis()) {
            // A successfully rebuilt solo cartridge resumes *in place*.
            // A failed conversion remains in the fail-closed Eject menu.
            self.overlay = if self.game.as_ref().is_some_and(GameSession::requires_relaunch) {
                ShelfOverlay::GameMenu { row: 0 }
            } else { ShelfOverlay::None };
        }
    }

    fn handle(&mut self, input: Input) {
        match input {
            Input::LinkPeerRomConsent(choice) => {
                if let Some(starter) = self.link_starter.as_ref() {
                    let decision = match choice {
                        1 => crate::trusted_link::PeerPermission::AllowOnce,
                        2 => crate::trusted_link::PeerPermission::Remember,
                        _ => crate::trusted_link::PeerPermission::Reject,
                    };
                    starter.approve_peer_rom(decision);
                    if choice != 1 && choice != 2 {
                        self.cancel_link_discovery();
                        self.fail_multiplayer("You declined sharing ROM data with this peer",
                            LinkFail::Cancelled);
                    }
                }
            }
            Input::LinkNetworkReady(address) => {
                if matches!(self.overlay, ShelfOverlay::Multiplayer) &&
                    matches!(self.link_phase, LinkMenu::Working {..}) {
                    let start = address.parse::<Ipv4Addr>()
                        .map_err(|_| "Invalid Wi-Fi IPv4 address".to_string())
                        .and_then(|ip| {
                            let config = self.link_config()?;
                            let role = if self.link_role == LinkRow::Host {
                                LinkRole::Host
                            } else { LinkRole::Join };
                            LinkStarter::spawn(role, ip, config).map_err(|e| e.to_string())
                        });
                    match start {
                        Ok(starter) => self.link_starter = Some(starter),
                        Err(e) => self.fail_multiplayer(&e, LinkFail::Radio),
                    }
                } else {
                    LINK_NEEDS_MULTICAST.store(false, Ordering::Release);
                }
            }
            Input::LinkNetworkError(reason) => {
                if matches!(self.overlay, ShelfOverlay::Multiplayer) &&
                    matches!(self.link_phase, LinkMenu::Working {..}) {
                    self.fail_multiplayer(&reason, LinkFail::Radio);
                }
            }
            Input::RaAccountConfigured => {
                // A newly authenticated account may start a fresh worker.
                self.ra_signing_out = false;
                self.ensure_ra_service();
            }
            Input::RaPrefetchHash(hash, console) => {
                // In-flight Android SAF scans from the old account must not
                // relaunch native networking during a hot logout.
                if !self.ra_signing_out {
                    self.ensure_ra_service();
                    if let Some(service) = self.ra_service.as_ref() {
                        service.prefetch_rom_hash_for_console(&hash, console);
                    }
                }
            }
            Input::RaNetworkAvailable => {
                if let Some(service) = self.ra_service.as_ref() {
                    service.network_available();
                }
            }
            Input::ReloadVisuals => {
                // Triggered from Android SAF, applied on the GL thread only.
                let root = PATHS.lock().unwrap_or_else(|e| e.into_inner())
                    .as_ref().map(|(root, _)| root.clone());
                if let Some(root) = root {
                    slot_ui::set_theme(slot_store::Theme::read(&root));
                    let custom_colours=root.join("Config/theme.txt").is_file();
                    self.slot_faces=if custom_colours { None }
                        else if self.settings.appearance == crate::settings::Appearance::Light {
                            Some(self.slot_light_faces)
                        } else { Some(self.slot_dark_faces) };
                    let was_custom_wallpaper = self.custom_wallpaper_present;
                    self.custom_wallpaper_present = root.join("Wallpapers/user.png").is_file();
                    if let ShelfOverlay::InterfaceSettings { row } = self.overlay {
                        if !self.custom_wallpaper_present && row > 3 {
                            self.overlay = ShelfOverlay::InterfaceSettings { row: 3 };
                        }
                    }
                    // A freshly imported user.png should appear immediately.
                    // Explicit None selection is never overridden by import.
                    if self.custom_wallpaper_present &&
                        !was_custom_wallpaper &&
                        self.settings.wallpaper != crate::settings::WallpaperPick::None {
                        self.settings.wallpaper =
                            crate::settings::WallpaperPick::File("user.png".into());
                        self.persist_settings();
                    }
                    // A deleted custom image falls back to Default only if
                    // it was the current selection.
                    if let crate::settings::WallpaperPick::File(name) = &self.settings.wallpaper {
                        if !crate::wallpaper::list(&root).contains(name) {
                            self.settings.wallpaper = crate::settings::WallpaperPick::Default;
                            self.persist_settings();
                        }
                    }
                    self.reload_wallpaper(Some(&root));
                    self.refresh_wallpaper_name();
                }
            }
            Input::ReloadCartLabel { uri } => {
                for shelf in &self.shelves {
                    for (index, cart) in shelf.carts.iter().enumerate() {
                        if cart.rom.to_string_lossy() == uri {
                            if let Some(tex) = shelf.face(index) {
                                let face = cart_face_with_material(cart, None);
                                self.gpu.update_texture(tex, face.w, face.h, &face.rgba);
                            }
                        }
                    }
                }
            }
            Input::ExternalReturn => {
                if self.external_pending {
                    self.external_pending = false;
                    // Keep the original Menu and selected row when coming back
                    // from Cart Studio, Apps or the RetroAchievements panel.
                }
            }
            Input::Reset => {
                self.reset_time_controls();
                self.buttons = 0;
                self.mode_down_at = None;
                self.mode_last_tap = None;
                self.home_launch_held = false;
                self.select_home_consumed = false;
                self.a_down_at = None;
                self.polaroids = None;
                // Android calls Reset onPause. Retain the *native menu* behind
                // Apps/Studio while the new Activity animates in, otherwise
                // the user briefly sees the cartridge carousel again.
                if !self.external_pending {
                    self.overlay = if self.game.as_ref().is_some_and(GameSession::requires_relaunch) {
                        ShelfOverlay::GameMenu { row: 0 }
                    } else { ShelfOverlay::None };
                }
                self.picker = None;
                for shelf in &mut self.shelves {
                    shelf.release_hold();
                }
            }
            Input::Suspend => {
                self.cancel_link_discovery();
                if let Some(session) = self.game.as_mut() { session.end_multiplayer(); }
                self.hud.set_link(LinkBadge::Off);
                // Android can go to sleep while a linked game is fullscreen,
                // not just while its Multiplayer menu is visible. The peer
                // has now been disconnected and continuing in-game would
                // accidentally run a private Link core as SOLO. Always
                // resume through the safe Eject-to-solo menu in this case.
                if suspend_returns_to_eject_menu(
                    self.game.as_ref().is_some_and(GameSession::requires_relaunch),
                    matches!(self.overlay, ShelfOverlay::Multiplayer)) {
                    self.overlay = ShelfOverlay::GameMenu { row: 0 };
                    self.game_accum = 0.0;
                }
                if let Some(session) = self.game.as_mut() {
                    if session.save(true) && !session.is_link_sandbox() {
                        if let Some(uri) = self.requested_uri.as_deref() {
                            let paths = PATHS.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let core = paths.as_ref().map(|(storage, _)| {
                                core_selection::selected(storage, uri).as_str()
                            }).unwrap_or("mgba");
                            let event = serde_json::json!({"uri": uri, "core": core}).to_string();
                            SAVE_SYNC.lock().unwrap_or_else(|e| e.into_inner()).push_back(event);
                        }
                    }
                }
                self.buttons = 0;
                AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
            }
            Input::Exit => {
                self.hud.set_link(LinkBadge::Off);
                self.cancel_link_discovery();
                self.link_identity = None;
                self.reset_time_controls();
                self.pending_game = None;
                // Preserve the final video texture for the original 160ms CRT
                // power-off before the mechanical 450ms cartridge ejection.
                if self.game.is_some() {
                    self.exiting_screen = true;
                    self.eject_hold_remaining = EJECT_HOLD_S;
                }
                // The reversed cart animation will emit the original eject sound.
                if self.progress > 0.0 || self.inserted { self.eject_sound_armed = true; }
                if let Some(mut session) = self.game.take() {
                    let saved = session.save(self.live_game_options().eject_save);
                    if saved && !session.is_link_sandbox() {
                        if let Some(uri) = self.requested_uri.as_deref() {
                            let paths = PATHS.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let core = paths.as_ref().map(|(storage, _)| {
                                core_selection::selected(storage, uri).as_str()
                            }).unwrap_or("mgba");
                            let event = serde_json::json!({"uri": uri, "core": core}).to_string();
                            SAVE_SYNC.lock().unwrap_or_else(|e| e.into_inner()).push_back(event);
                        }
                    }
                }
                PLAYING.store(false, Ordering::Release);
                SAMPLE_RATE.store(0, Ordering::Release);
                AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
                self.buttons = 0;
                self.awaiting_game = false;
                self.inserted = false;
                self.overlay = ShelfOverlay::None;
                self.picker = None;
                self.a_down_at = None;
                self.home_launch_held = false;
                self.fresh_launch = false;
                self.mode_down_at = None;
                self.mode_last_tap = None;
                self.select_home_consumed = false;
                self.polaroids = None;
                self.requested_uri = None;
                self.prepared_rom = None;
                self.link_identity = None;
                self.link_mode_override = None;
                self.game_emulation = None;
                *REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Input::ShelfStatus(code) => {
                self.shelf_status = if usize::from(code) < SHELF_STATUS_TEXT.len() {
                    code
                } else { 0 };
            }
            Input::GameError { uri, message } => {
                if !self.inserted || !self.awaiting_game
                    || self.requested_uri.as_deref() != Some(uri.as_str())
                {
                    return;
                }
                set_message(format!("Cannot open game: {message}"));
                self.awaiting_game = false;
                self.inserted = false;
                self.requested_uri = None;
                self.prepared_rom = None;
            }
            Input::GameReady { uri, path } => {
                // ROM preparation is asynchronous: a previously cancelled cart must
                // never launch after selecting another cartridge.
                if self.awaiting_game && self.inserted
                    && self.requested_uri.as_deref() == Some(uri.as_str())
                {
                    self.awaiting_game = false;
                    self.prepared_rom = Some(path);
                }
            }
            Input::Key(code, pressed) => {
                if matches!(code, 19 | 20) {
                    if !pressed {
                        if self.menu_held.is_some_and(|(held,_,_)|held==code) {
                            self.menu_held=None;
                        }
                    } else if !self.menu_repeat_dispatch
                        && !matches!(self.overlay,ShelfOverlay::None)
                    {
                        self.menu_held=Some((code,self.born.elapsed().as_millis() as u64+400,0));
                    }
                } else if pressed && matches!(code,96|97|108) {
                    self.menu_held=None;
                }
                if code == 110 {
                    match select_home_action(
                        pressed,
                        self.select_down_at.is_some(),
                        self.game.is_some(),
                        matches!(self.overlay, ShelfOverlay::None | ShelfOverlay::GameMenu { .. }),
                        &mut self.select_home_consumed,
                    ) {
                        SelectHomeAction::Swallow => return,
                        SelectHomeAction::OpenLink => {
                            // The SELECT+HOME gesture belongs to Slot, not to
                            // libretro or Android's normal HOME handler.
                            self.select_down_at = None;
                            self.select_chorded = true;
                            self.select_forwarded = false;
                            self.select_tap_frames = 0;
                            self.buttons = 0;
                            self.mode_down_at = None;
                            self.mode_last_tap = None;
                            self.game_accum = 0.0;
                            if self.game.as_ref().is_some_and(GameSession::linked) {
                                self.link_entry = LinkEntry::Shortcut;
                                // Upstream SELECT+MENU reopens the animated
                                // linked adapter and A=End Link / B=Back.
                                self.link_phase = LinkMenu::Linked {
                                    role: self.link_role,
                                    worked: self.link_worked_at,
                                    since: self.link_millis(),
                                    opened: true,
                                };
                                self.overlay = ShelfOverlay::Multiplayer;
                            } else {
                                self.enter_multiplayer_menu();
                                if matches!(self.overlay, ShelfOverlay::Multiplayer) {
                                    self.link_entry = LinkEntry::Shortcut;
                                }
                            }
                            return;
                        }
                        SelectHomeAction::Pass => {}
                    }
                }
                if code == 110 && matches!(self.overlay, ShelfOverlay::Multiplayer) {
                    if !pressed {
                        if matches!(self.link_phase, LinkMenu::Linked { opened: true, .. }) {
                            self.overlay = ShelfOverlay::None;
                        } else if !matches!(self.link_phase, LinkMenu::Unplug { .. }) {
                            self.cancel_link_discovery();
                            self.overlay = self.link_entry.back(
                                        self.game.as_ref().is_some_and(GameSession::linked),
                                        self.game.as_ref().is_some_and(GameSession::requires_relaunch));
                        }
                    }
                    return;
                }
                // KONKR's top-round controller key emits Linux BTN_MODE. Android
                // normally delivers that as KEYCODE_BUTTON_MODE (110); this is
                // separate from the system HOME launcher key.
                if code == 110 {
                    // On the carousel ONLY, Home/MODE mirrors A's insert and
                    // 400ms fresh-launch logic. Consumed releases never enter
                    // the game menu or the system launcher.
                    if self.home_launch_held {
                        if !pressed {
                            self.home_launch_held = false;
                            if self.game.is_none() {
                                self.handle(Input::Key(96, false));
                            } else {
                                self.a_down_at = None;
                            }
                        }
                        return;
                    }
                    if pressed && self.game.is_none() && !self.inserted
                        && matches!(self.overlay, ShelfOverlay::None)
                        && self.shelves.get(self.active).is_some_and(|s| !s.carts.is_empty())
                    {
                        self.home_launch_held = true;
                        self.handle(Input::Key(96, true));
                        return;
                    }
                    if pressed {
                        if self.mode_down_at.is_none() {
                            self.mode_down_at = Some(Instant::now());
                        }
                    } else if let Some(since) = self.mode_down_at.take() {
                        if since.elapsed().as_millis() >= 650 {
                            self.mode_last_tap = None;
                            if self.game.is_some() { self.handle(Input::Exit); }
                        } else if self.game.as_ref().is_some_and(GameSession::requires_relaunch) {
                            self.mode_last_tap = None;
                            self.overlay = ShelfOverlay::GameMenu { row: 0 };
                            set_message("Press A to eject and relaunch in solo".into());
                        } else if self.game.is_some() {
                            let is_double=self.mode_last_tap.take()
                                .is_some_and(|t|t.elapsed().as_millis()<360);
                            self.buttons=0;
                            self.game_accum=0.0;
                            AUDIO.lock().unwrap_or_else(|e|e.into_inner()).clear();
                            self.reset_time_controls();
                            RUMBLE_STRENGTH.store(0, Ordering::Release);
                            if is_double {
                                self.open_polaroids();
                            } else {
                                self.mode_last_tap=Some(Instant::now());
                                self.overlay=match self.overlay {
                                    ShelfOverlay::GameMenu { .. }
                                    | ShelfOverlay::PauseSettings { .. }
                                    | ShelfOverlay::PauseScreenSettings { .. }
                                    | ShelfOverlay::PauseGameplaySettings { .. }
                                    | ShelfOverlay::PauseEmulationSettings { .. }
                                    | ShelfOverlay::States => ShelfOverlay::None,
                                    _=>ShelfOverlay::GameMenu {row:0},
                                };
                            }
                        }
                        // START remains the ONLY menu key on the carousel.
                    }
                    return;
                }
                if self.game.is_some() && matches!(self.overlay, ShelfOverlay::Multiplayer) {
                    if pressed {
                        match self.link_phase {
                            LinkMenu::Pick(_) => match code {
                                109 if self.link_mode_switchable() => {
                                    self.link_mode = self.link_mode.other();
                                    self.link_mode_override = Some(self.link_mode);
                                    // The actual gpSP option is applied by an
                                    // atomic core reload before linking.
                                }
                                21 | 22 => {
                                    self.link_role = if self.link_role == LinkRow::Host {
                                        LinkRow::Join
                                    } else { LinkRow::Host };
                                    self.link_phase = LinkMenu::Pick(self.link_role);
                                }
                                96 => self.request_multiplayer_network(),
                                97 | 108 => {
                                    self.cancel_link_discovery();
                                    self.overlay = self.link_entry.back(
                                        self.game.as_ref().is_some_and(GameSession::linked),
                                        self.game.as_ref().is_some_and(GameSession::requires_relaunch));
                                }
                                _ => {}
                            },
                            LinkMenu::Working { .. } => {
                                if code == 97 || code == 108 {
                                    self.cancel_link_discovery();
                                    self.overlay = self.link_entry.back(
                                        self.game.as_ref().is_some_and(GameSession::linked),
                                        self.game.as_ref().is_some_and(GameSession::requires_relaunch));
                                }
                            }
                            LinkMenu::Failed { .. } => {
                                if code == 96 || code == 97 || code == 108 {
                                    self.overlay = self.link_entry.back(
                                        self.game.as_ref().is_some_and(GameSession::linked),
                                        self.game.as_ref().is_some_and(GameSession::requires_relaunch));
                                }
                            }
                            LinkMenu::Linked { opened: true, role, .. } => match code {
                                96 => {
                                    // End Link is a virtual unplug, not a
                                    // cartridge ejection. Solo save states and
                                    // Polaroids become available ONLY after
                                    // a verified one-GBA recovery + promotion.
                                    match self.return_link_to_solo() {
                                        Ok(()) => {
                                            self.hud.set_link(LinkBadge::Off);
                                            // The physical unplug animation is the success cue.
                                            // No additional transient toast over gameplay.
                                            self.link_phase = LinkMenu::Unplug {
                                                role, since: self.link_millis(),
                                            };
                                            self.game_accum = 0.0;
                                            self.buttons = 0;
                                        }
                                        Err(reason) => {
                                            // Never continue in a stale Link
                                            // core and never overwrite solo
                                            // progress following recovery error.
                                            if let Some(game) = self.game.as_mut() {
                                                game.end_multiplayer();
                                            }
                                            set_message(format!("Link recovery unsafe: {reason}. Original solo progress preserved."));
                                            self.hud.set_link(link_badge_for(role, true));
                                            if self.game.is_some() {
                                                self.overlay = ShelfOverlay::GameMenu { row: 0 };
                                            } else {
                                                self.handle(Input::Exit);
                                            }
                                        }
                                    }
                                }
                                97 | 108 => self.overlay = self.link_entry.back(
                                        self.game.as_ref().is_some_and(GameSession::linked),
                                        self.game.as_ref().is_some_and(GameSession::requires_relaunch)),
                                _ => {}
                            },
                            LinkMenu::Linked { opened: false, .. } | LinkMenu::Unplug { .. } => {}
                        }
                    }
                    return;
                }
                if self.game.is_some() && matches!(self.overlay, ShelfOverlay::States) {
                    if pressed {
                        match code {
                            21 => {
                                if let Some(p) = self.polaroids.as_mut() { p.left(); }
                                self.update_polaroid_title();
                            }
                            22 => {
                                if let Some(p) = self.polaroids.as_mut() { p.right(); }
                                self.update_polaroid_title();
                            }
                            96 => {
                                if let (Some(game),Some(p))=(self.game.as_mut(),self.polaroids.as_ref()) {
                                    if let Some(entry)=p.selected() {
                                        if let Err(err)=game.load_manual(&entry.stamp) {
                                            set_message(format!("State load failed: {err}"));
                                        }
                                    }
                                }
                                self.overlay=ShelfOverlay::None;
                            }
                            100 => { // Y deletes one Slot history entry only.
                                if let (Some(game),Some(p))=(self.game.as_mut(),self.polaroids.as_mut()) {
                                    if let Some(entry)=p.selected() {
                                        if let Err(err)=game.delete_manual(&entry.stamp) {
                                            set_message(format!("State delete failed: {err}"));
                                        } else {
                                            p.remove_selected();
                                        }
                                    }
                                }
                                if self.polaroids.as_ref().is_some_and(|p|p.is_empty()) {
                                    self.overlay=ShelfOverlay::None;
                                } else {
                                    self.update_polaroid_title();
                                }
                            }
                            99 => { // X: original Slot's 30-second undo.
                                if let Some(game)=self.game.as_mut() {
                                    if let Err(err)=game.undo_manual() { set_message(err); }
                                }
                                self.overlay=ShelfOverlay::None;
                            }
                            97=>self.overlay=ShelfOverlay::None,
                            _=>{}
                        }
                    }
                    return;
                }
        if self.game.is_some() && matches!(self.overlay, ShelfOverlay::GameMenu { .. }) {
                    if pressed {
                        let linked = self.game.as_ref().is_some_and(GameSession::linked);
                        let relaunch = self.game.as_ref().is_some_and(GameSession::requires_relaunch);
                        if let ShelfOverlay::GameMenu { row } = self.overlay {
                            match code {
                                19 | 20 => self.overlay = ShelfOverlay::GameMenu {
                                    row: move_menu_row(row, code, pause_rows(linked, relaunch).len()),
                                },
                                96 => match pause_rows(linked, relaunch).get(row).copied() {
                                    Some(PauseRow::SaveEject) => self.handle(Input::Exit),
                                    Some(PauseRow::Resume) => self.overlay = ShelfOverlay::None,
                                    Some(PauseRow::States) if !linked && !relaunch =>
                                        self.open_polaroids(),
                                    Some(PauseRow::Multiplayer) => {
                                        self.link_entry = LinkEntry::GameMenu;
                                        if linked {
                                            // Show existing connected adapter, never unplug
                                            // when merely opening the Multiplayer menu.
                                            self.link_phase = LinkMenu::Linked {
                                                role: self.link_role,
                                                worked: self.link_worked_at,
                                                since: self.link_millis(), opened: true,
                                            };
                                            self.overlay = ShelfOverlay::Multiplayer;
                                        } else { self.enter_multiplayer_menu(); }
                                    },
                                    Some(PauseRow::Settings) if !relaunch =>
                                        self.overlay = ShelfOverlay::PauseSettings { row: 0 },
                                    _ => {}
                                },
                                97 => if !relaunch {
                                    self.overlay = ShelfOverlay::None
                                } else { set_message("Eject to return to solo.".into()); },
                                _ => {}
                            }
                        }
                    }
                    return;
                }
                if self.game.is_some() && matches!(self.overlay, ShelfOverlay::PauseSettings { .. }
                    | ShelfOverlay::PauseScreenSettings { .. }
                    | ShelfOverlay::PauseGameplaySettings { .. }
                    | ShelfOverlay::PauseEmulationSettings { .. }) {
                    if pressed {
                        match self.overlay {
                            ShelfOverlay::PauseSettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::PauseSettings {
                                    row: move_menu_row(row, code, self.pause_settings_rows().len()),
                                },
                                96 => {
                                    let linked=self.game.as_ref().is_some_and(GameSession::linked);
                                    self.overlay=match row {
                                        0 => ShelfOverlay::PauseScreenSettings { row: 0 },
                                        1 if !linked => ShelfOverlay::PauseGameplaySettings { row: 0 },
                                        2 if !linked => ShelfOverlay::PauseEmulationSettings { row: 0 },
                                        3 if !linked => {self.reset_game_emulation();
                                            ShelfOverlay::PauseSettings { row }},
                                        _ => ShelfOverlay::PauseSettings { row },
                                    };
                                },
                                97 => self.overlay = ShelfOverlay::GameMenu {
                                    row: pause_rows(self.game.as_ref().is_some_and(GameSession::linked),
                                        self.game.as_ref().is_some_and(GameSession::requires_relaunch))
                                        .iter().position(|item| *item==PauseRow::Settings).unwrap_or(0),
                                },
                                _ => {}
                            },
                            ShelfOverlay::PauseScreenSettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::PauseScreenSettings {
                                    row: move_menu_row(row,code,self.pause_screen_rows().len()),
                                },
                                21 | 22 => if let Some(key)=self.pause_screen_key(row) {
                                    self.change_game_screen_or_gameplay(key,code==22);
                                },
                                97 => self.overlay = ShelfOverlay::PauseSettings { row: 0 },
                                _ => {}
                            },
                            ShelfOverlay::PauseGameplaySettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::PauseGameplaySettings {
                                    row: move_menu_row(row,code,6),
                                },
                                21 | 22 => self.change_game_screen_or_gameplay(row+4,code==22),
                                97 => self.overlay = ShelfOverlay::PauseSettings { row: 1 },
                                _ => {}
                            },
                            ShelfOverlay::PauseEmulationSettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::PauseEmulationSettings {
                                    row: move_menu_row(row, code, 3),
                                },
                                21 | 22 => self.change_game_emulation(row + 10, code == 22),
                                97 => self.overlay = ShelfOverlay::PauseSettings { row: 2 },
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                    return;
                }

                if self.game.is_some() {
                    // Defer SELECT to reserve chord handling, as in upstream Gestures.
                    if code == 109 {
                        if pressed {
                            self.select_down_at = Some(Instant::now());
                            self.select_chorded = false;
                            self.select_forwarded = false;
                            self.select_tap_frames = 0;
                            self.buttons &= !ButtonMask::SELECT;
                        } else {
                            if self.select_down_at.take().is_some() && !self.select_chorded {
                                if self.select_forwarded {
                                    self.buttons &= !ButtonMask::SELECT;
                                } else {
                                    self.buttons |= ButtonMask::SELECT;
                                    self.select_tap_frames = SELECT_TAP_FRAMES;
                                }
                            } else { self.buttons &= !ButtonMask::SELECT; }
                            self.select_chorded = false;
                            self.select_forwarded = false;
                        }
                        return;
                    }
                    if self.handle_select_chord(code, pressed) { return; }
                    if pressed && self.select_down_at.is_some() && !self.select_chorded {
                        self.select_forwarded = true;
                        self.buttons |= ButtonMask::SELECT;
                    }
                    // Linked frames are ordered by both players. Never allow
                    // local time travel or accelerated single-player frames.
                    if self.game.as_ref().is_some_and(GameSession::linked)
                        && (code == 104 || code == 105) { return; }
                    if code == 104 {
                        if !self.live_game_options().rewind { return; }
                        if pressed {
                            self.rewind_held = true;
                            self.rewind_accum = 0.0;
                            self.ff_held = false;
                            self.ff_latched = false;
                            self.ff_last_release = None;
                            self.hud.set_ff(FfState::Off);
                            AUDIO_SPEED_PERMILLE.store(1000, Ordering::Release);
                            AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        } else {
                            self.rewind_held = false;
                            self.rewind_accum = 0.0;
                            self.game_accum = 0.0;
                            self.hud.release_rewind();
                            AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        }
                        return;
                    }
                    if code == 105 {
                        if pressed {
                            if self.ff_latched {
                                self.ff_latched = false;
                                self.ff_held = false;
                                self.ff_last_release = None;
                            } else {
                                let double = self.ff_last_release.take().is_some_and(
                                    |at| at.elapsed().as_millis() <= FF_DOUBLE_TAP_MS);
                                self.ff_latched = double;
                                self.ff_held = true;
                            }
                            self.rewind_held = false;
                            self.hud.release_rewind();
                            AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        } else if self.ff_held {
                            self.ff_held = false;
                            self.ff_last_release = Some(Instant::now());
                        }
                        AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        AUDIO_SPEED_PERMILLE.store(
                            if self.ff_latched || self.ff_held {
                                i32::from(self.live_game_options().ff_speed) * 1000
                            } else { 1000 },
                            Ordering::Release);
                        self.hud.set_ff(match (self.ff_latched, self.ff_held) {
                            (true, _) => FfState::Latched,
                            (false, true) => FfState::Held,
                            (false, false) => FfState::Off,
                        });
                        return;
                    }
                    // Slot original: GB/GBC L1 = Stretch, R1 = Actual.
                    // On GBA the shoulder buttons remain owned by the core.
                    // SELECT+shoulder save/load is handled below and takes priority.
                    if self.current_cart_platform() != Platform::Gba && self.buttons & ButtonMask::SELECT == 0
                        && (code == 102 || code == 103) {
                        if pressed {
                            self.set_video_mode(if code == 102 {
                                VideoMode::Stretch
                            } else { VideoMode::Actual });
                        }
                        return;
                    }
                    let mask = controls(code);
                    if pressed { self.buttons |= mask; } else { self.buttons &= !mask; }
                    return;
                }
                if self.overlay != ShelfOverlay::None {
                    if pressed {
                        match self.overlay {
                            ShelfOverlay::Menu { row } => match code {
                                19 | 20 => {
                                    self.overlay = ShelfOverlay::Menu { row: move_menu_row(row, code, 6) };
                                }
                                // Debounce taps while Apps, Studio or RA is opening.
                                96 if !self.external_pending => {
                                    self.overlay = match row {
                                        0 => ShelfOverlay::Library { row: 0 },
                                        1 => {
                                            // Show the entire indexed GB/GBC/GBA library.
                                            UI_ACTION.lock().unwrap_or_else(|e| e.into_inner())
                                                .push_back(14);
                                            self.external_pending = true;
                                            ShelfOverlay::Menu { row }
                                        },
                                        2 => {
                                            // Keep Menu rendered behind the RA dialog until
                                            // Android has actually shown it, just like Apps.
                                            // Closing the native overlay immediately flashes
                                            // the carousel before Dialog.show() is reached.
                                            UI_ACTION.lock().unwrap_or_else(|e| e.into_inner())
                                                .push_back(3);
                                            self.external_pending = true;
                                            ShelfOverlay::Menu { row }
                                        },
                                        3 => {
                                            UI_ACTION.lock().unwrap_or_else(|e| e.into_inner()).push_back(15);
                                            self.external_pending = true;
                                            ShelfOverlay::Menu { row }
                                        },
                                        4 => ShelfOverlay::Settings { row: 0 },
                                        _ => {
                                            self.prepare_about();
                                            ShelfOverlay::About
                                        },
                                    };
                                }
                                97 | 108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::Library { row } => match code {
                                19 | 20 => {
                                    let next = if code == 19 { (row + 5) % 6 } else { (row + 1) % 6 };
                                    self.overlay = ShelfOverlay::Library { row: next };
                                }
                                // Identical left/right + A toggle to Settings.
                                // Keep the selected row and refresh value in place.
                                21 | 22 | 96 if row == 4 => {
                                    UI_ACTION.lock().unwrap_or_else(|e| e.into_inner())
                                        .push_back(17);
                                }
                                96 => match row {
                                    0..=5 => {
                                        // ROM, BIOS, SRAM root, State root,
                                        // Share Saves OFF/ON, Refresh.
                                        let action = match row {
                                            0 => 1,
                                            1 => 4,
                                            2 => 5,
                                            3 => 6,
                                            4 => 17,
                                            _ => 2,
                                        };
                                        UI_ACTION.lock().unwrap_or_else(|e| e.into_inner())
                                            .push_back(action);
                                        self.overlay = ShelfOverlay::None;
                                    }
                                    _ => {}
                                },
                                97 => self.overlay = ShelfOverlay::Menu { row: 0 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::Settings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::Settings {
                                    row: move_menu_row(row, code, 5),
                                },
                                96 => match row {
                                    0 => self.overlay = ShelfOverlay::GeneralSettings { row: 0 },
                                    1 => self.overlay = ShelfOverlay::ScreenSettings { row: 0 },
                                    2 => self.overlay = ShelfOverlay::GameplaySettings { row: 0 },
                                    3 => self.overlay = ShelfOverlay::EmulationSettings { row: 0 },
                                    4 => self.overlay = ShelfOverlay::InterfaceSettings { row: 0 },
                                    _ => return,
                                },
                                97 => self.overlay = ShelfOverlay::Menu { row: 4 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::GeneralSettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::GeneralSettings {
                                    row: move_menu_row(row, code, self.general_rows().len()),
                                },
                                21 | 22 | 96 => {
                                    self.cycle_general_row(row,code!=21,code==96);
                                },
                                97 => self.overlay = ShelfOverlay::Settings { row: 0 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::InterfaceSettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::InterfaceSettings {
                                    row: move_menu_row(row, code, 3),
                                },
                                21 | 22 if row < 3 => self.change_setting(row + 13, code == 22),
                                97 => self.overlay = ShelfOverlay::Settings { row: 4 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::ScreenSettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::ScreenSettings {
                                    row: move_menu_row(row, code, 3),
                                },
                                96 => self.overlay=ShelfOverlay::ScreenSystemSettings {
                                    platform:Self::screen_submenu_platform(row),row:0,
                                },
                                97 => self.overlay = ShelfOverlay::Settings { row: 1 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::ScreenSystemSettings { platform,row } => match code {
                                19 | 20 => self.overlay=ShelfOverlay::ScreenSystemSettings {
                                    platform,row:move_menu_row(row,code,Self::screen_rows(platform).len()),
                                },
                                21 | 22 | 96 => if let Some(key)=Self::screen_keys(platform).get(row).copied(){
                                    self.change_setting(key,code!=21);
                                },
                                97 => self.overlay=ShelfOverlay::ScreenSettings {
                                    row:match platform{Platform::Gba=>0,Platform::Gbc=>1,Platform::Gb=>2},
                                },
                                108 => self.overlay=ShelfOverlay::None,
                                _=>{}
                            },
                            ShelfOverlay::EmulationSettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::EmulationSettings {
                                    row: move_menu_row(row, code, 3),
                                },
                                21 | 22 => self.change_setting(row + 10, code == 22),
                                97 => self.overlay = ShelfOverlay::Settings { row: 3 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::GameplaySettings { row } => match code {
                                19 | 20 => self.overlay = ShelfOverlay::GameplaySettings {
                                    row: move_menu_row(row, code, 6),
                                },
                                21 | 22 => self.change_setting(row + 4, code == 22),
                                97 => self.overlay = ShelfOverlay::Settings { row: 2 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::About => match code {
                                97 | 96 => self.overlay = ShelfOverlay::Menu { row: 5 },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::Scraping | ShelfOverlay::Achievements => match code {
                                96 if matches!(self.overlay, ShelfOverlay::Scraping) => {
                                    if let Some(cart) = self.shelves[self.active].carts
                                        .get(self.shelves[self.active].index) {
                                        *LABEL_PICK_REQUEST.lock()
                                            .unwrap_or_else(|e| e.into_inner()) =
                                            Some(cart.rom.to_string_lossy().into_owned());
                                        UI_ACTION.lock().unwrap_or_else(|e| e.into_inner())
                                            .push_back(13);
                                    } else {
                                        set_message("Choose a cartridge first".into());
                                    }
                                }
                                96 if matches!(self.overlay, ShelfOverlay::Achievements) => {
                                    UI_ACTION.lock().unwrap_or_else(|e| e.into_inner()).push_back(3);
                                    self.external_pending = true;
                                }
                                97 => self.overlay = ShelfOverlay::Menu {
                                    row: if matches!(self.overlay, ShelfOverlay::Scraping) { 1 } else { 2 },
                                },
                                108 => self.overlay = ShelfOverlay::None,
                                _ => {}
                            },
                            ShelfOverlay::Core => {
                                let now = self.born.elapsed().as_millis() as u64;
                                if let Some(picker) = self.picker.as_mut() {
                                    match code {
                                        21 => { picker.press(Press::Left, now); }
                                        22 => { picker.press(Press::Right, now); }
                                        96 => {
                                            let core = picker.seat();
                                            let uri = self.shelves[self.active].carts
                                                [self.shelves[self.active].index]
                                                .rom.to_string_lossy().into_owned();
                                            let paths = PATHS.lock()
                                                .unwrap_or_else(|e| e.into_inner()).clone();
                                            if let Some((storage, library)) = paths {
                                                let core_file = library.join(
                                                    format!("lib{}_libretro.so", core.as_str()));
                                                if !core_file.is_file() {
                                                    set_message(format!(
                                                        "{} core is not installed", core.text()));
                                                } else {
                                                    match core_selection::set(&storage, &uri, core) {
                                                        Ok(()) => {
                                                            // A core swap must invalidate the
                                                            // previous core's cached profile
                                                            // immediately, even when Y was
                                                            // opened before selecting this chip.
                                                            self.game_emulation =
                                                                EmulationProfile::load(
                                                                    &storage, &uri, core);
                                                            picker.press(Press::Keep, now);
                                                        }
                                                        Err(err) => set_message(format!(
                                                            "Cannot save core selection: {err}")),
                                                    }
                                                }
                                            }
                                        }
                                        97 | 109 => { picker.press(Press::Back, now); }
                                        _ => {}
                                    }
                                }
                            },
                            ShelfOverlay::GameMenu { .. } | ShelfOverlay::Multiplayer |
                            ShelfOverlay::ScreenSystemSettings { .. } |
                            ShelfOverlay::PauseSettings { .. } | ShelfOverlay::PauseScreenSettings { .. } |
                            ShelfOverlay::PauseGameplaySettings { .. } | ShelfOverlay::PauseEmulationSettings { .. } |
                            ShelfOverlay::States | ShelfOverlay::None => {}
                        }
                    }
                    return;
                }
                if pressed {
                    match code {
                        // X edits the current cartridge directly without insertion.
                        99 if !self.inserted && !self.shelves[self.active].carts.is_empty() => {
                            let cart = &self.shelves[self.active].carts
                                [self.shelves[self.active].index];
                            *LABEL_PICK_REQUEST.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(cart.rom.to_string_lossy().into_owned());
                            UI_ACTION.lock().unwrap_or_else(|e| e.into_inner()).push_back(13);
                            return;
                        }
                        // Original Slot v1.7: Y silently toggles the star.
                        // All per-ROM settings remain in Pause -> Settings.
                        100 if !self.inserted => {
                            self.toggle_favorite();
                            return;
                        }
                        108 if !self.inserted => {
                            self.shelves[self.active].release_hold();
                            self.overlay = ShelfOverlay::Menu { row: 0 };
                            return;
                        }
                        109 if !self.inserted && !self.shelves[self.active].carts.is_empty() => {
                            let selected = &self.shelves[self.active].carts[self.shelves[self.active].index];
                            if selected.platform != Platform::Gba {
                                self.refusal = Some(slot_ui::Refusal::started(
                                    self.born.elapsed().as_millis() as u64));
                                set_message("mGBA is the only installed GB/GBC core".into());
                                return;
                            }
                            let face = board_face(selected);
                            if let Some(texture) = self.board_texture {
                                self.gpu.update_texture(texture, face.w, face.h, &face.rgba);
                            } else {
                                self.board_texture = Some(self.gpu.create_texture(face.w, face.h, &face.rgba));
                            }
                            let now = self.born.elapsed().as_millis() as u64;
                            let current_core = PATHS.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .as_ref()
                                .map(|(storage, _)| core_selection::selected(
                                    storage, &selected.rom.to_string_lossy()))
                                .unwrap_or(Core::Mgba);
                            let mut picker = CorePicker::open(current_core, now);
                            picker.start(now);
                            self.picker = Some(picker);
                            self.overlay = ShelfOverlay::Core;
                            return;
                        }
                        102 | 103 if !self.inserted => {
                            let delta = if code == 102 { -1 } else { 1 };
                            // An empty platform is never a visible stop.
                            let next = next_nonempty_shelf(&self.shelves, self.active, delta);
                            if next != self.active {
                                self.shelves[self.active].release_hold();
                                self.active = next;
                                self.progress = 0.0;
                                self.letter_shown_at = None;
                                self.platform_shown_at = Some(Instant::now());
                                self.last_cart_navigation = Instant::now();
                            }
                            return;
                        }
                        96 if !self.inserted && self.shelves[self.active].carts.is_empty() => {
                            self.refusal = Some(slot_ui::Refusal::started(
                                self.born.elapsed().as_millis() as u64));
                            return;
                        }
                        96 if !self.inserted && !self.shelves[self.active].carts.is_empty() => {
                            self.load_selected_emulation();
                            self.inserted = true;
                            let selected = &self.shelves[self.active].carts[self.shelves[self.active].index];
                            let uri = selected.rom.to_string_lossy();
                                        self.prepared_rom = None;
                            self.pending_game = None;
                            self.insert_started = Some(Instant::now());
                            self.a_down_at = self.insert_started;
                            self.fresh_launch = false;
                            if uri.starts_with("content://") {
                                let uri = uri.into_owned();
                                self.requested_uri = Some(uri.clone());
                                self.awaiting_game = true;
                                *REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(uri);
                            }
                            return;
                        }
                        97 => {
                            if self.inserted && self.progress > 0.0 {
                                self.eject_sound_armed = true;
                            }
                            self.inserted = false;
                            self.a_down_at = None;
                            self.fresh_launch = false;
                            self.awaiting_game = false;
                            self.prepared_rom = None;
                            self.pending_game = None;
                            self.requested_uri = None;
                                        *REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = None;
                            return;
                        }
                        _ => {}
                    }
                }
                if code == 96 && !pressed {
                    if let Some(when) = self.a_down_at.take() {
                        self.fresh_launch = when.elapsed().as_millis() >= 400;
                    }
                    return;
                }
                let ms = self.born.elapsed().as_millis() as u64;
                if !self.inserted && pressed && matches!(code,19|20|21|22) {
                    self.last_cart_navigation=Instant::now();
                }
                let shelf = &mut self.shelves[self.active];
                let before = shelf.index;
                match (code, pressed) {
                    (21, true) if !self.inserted => shelf.hold_left(ms),
                    (22, true) if !self.inserted => shelf.hold_right(ms),
                    (21, false) => shelf.release_left(),
                    (22, false) => shelf.release_right(),
                    (19, true) if !self.inserted => shelf.hold_up(ms),
                    (20, true) if !self.inserted => shelf.hold_down(ms),
                    (19, false) => shelf.release_up(),
                    (20, false) => shelf.release_down(),
                    _ => {}
                }
                let jumped= !self.inserted && pressed && (code==19 || code==20)
                    && shelf.index!=before;
                if jumped { self.show_selected_letter(); }
            }
        }
    }

    fn upload_clear_material(&mut self, key:(usize,usize), layers:[slot_ui::CartFace;3]) {
        let textures=if let Some(existing)=self.clear_material {
            for (tex,face) in existing.tex.iter().copied().zip(layers.into_iter()) {
                self.gpu.update_texture(tex,face.w,face.h,&face.rgba);
            }
            existing.tex
        } else {
            layers.map(|face|self.gpu.create_texture(face.w,face.h,&face.rgba))
        };
        self.clear_material=Some(ClearMaterial{key,tex:textures});
    }
    fn clear_layers(&self) -> ([Option<slot_gfx::TexId>;2],Option<slot_gfx::TexId>) {
        let selected=self.shelves.get(self.active).map(|s|(self.active,s.index));
        match (self.clear_material,selected) {
            (Some(layer),Some(key)) if layer.key==key =>
                ([Some(layer.tex[0]),Some(layer.tex[1])],Some(layer.tex[2])),
            _ => ([None,None],None),
        }
    }

    // Upstream Slot rasterizes its artwork on worker threads. Keep the same
    // separation on Android: no slow PNG decode/embossing on the GLES thread.
    // GPU texture uploads stay on their GL owner and are bounded per frame.
    fn prepare_visible(&mut self) {
        // During scrolling cap uploads to one per frame and never work on
        // background disk tasks on the OpenGL thread.
        let quiet=self.last_cart_navigation.elapsed()>=CART_IDLE_GRACE
            && !self.shelves[self.active].is_scrolling()
            && matches!(self.overlay,ShelfOverlay::None);
        let limit=if quiet {2} else {1};
        let upload_started=Instant::now();
        for _ in 0..limit {
            if upload_started.elapsed()>=CART_GPU_UPLOAD_BUDGET {break}
            let Some(((shelf_id,index),face,layers))=self.cart_render.ready() else {break};
            let Some(shelf)=self.shelves.get(shelf_id) else {continue};
            if index>=shelf.carts.len(){continue}
            if let Some(layers)=layers {
                if shelf_id==self.active && index==shelf.index {
                    self.upload_clear_material((shelf_id,index),layers);
                }
            }
            if self.shelves[shelf_id].face(index).is_some() {continue}
            let texture = if self.texture_cache.len() >= CART_TEXTURE_CACHE_CAPACITY {
                // Pin selected +/- one cart from each nonempty shelf (up to
                // 15 across GBA/GB/GBC/Recents/Favorites). Preserve warm art
                // while the 96-face LRU bounds shared GPU memory.
                let victim = self.texture_cache.iter().position(|(sid, index, _)| {
                    self.shelves.get(*sid).is_none_or(|shelf|
                        shelf.carts.is_empty() || shelf.index.abs_diff(*index) > 1)
                }).unwrap_or(0);
                let (old_shelf, old_index, tex) = self.texture_cache
                    .remove(victim).expect("nonempty texture pool");
                if let Some(shelf) = self.shelves.get_mut(old_shelf) {
                    shelf.take_face(old_index);
                }
                self.gpu.update_texture(tex, face.w, face.h, &face.rgba);
                tex
            } else {
                self.gpu.create_texture(face.w, face.h, &face.rgba)
            };
            self.shelves[shelf_id].set_face(index, texture);
            self.texture_cache.push_back((shelf_id, index, texture));
        }
        let shelf_id = self.active;
        let selected = self.shelves[shelf_id].index;
        // Priorities are recomputed every frame, so a rapid D-pad or L1/R1
        // turn promotes new visible carts ahead of stale background requests.
        // Order: selected, active +/-1, next/previous system, the rest.
        let mut priority: Vec<(usize,usize)> = selected_prefetch_entry(&self.shelves,shelf_id)
            .into_iter().collect();
        for delta in [-1isize,1] {
            let pos=(selected as isize+delta).rem_euclid(
                self.shelves[shelf_id].carts.len().max(1) as isize) as usize;
            if !self.shelves[shelf_id].carts.is_empty()
                && !priority.contains(&(shelf_id,pos)) {
                priority.push((shelf_id,pos));
            }
        }
        for direction in [1,-1] {
            let sid=next_nonempty_shelf(&self.shelves,shelf_id,direction);
            if sid!=shelf_id && !priority.contains(&(sid,self.shelves[sid].index)) {
                priority.push((sid,self.shelves[sid].index));
            }
        }
        for index in self.shelves[shelf_id].on_screen() {
            if !priority.contains(&(shelf_id,index)) {
                priority.push((shelf_id,index));
            }
        }
        for direction in [1,-1] {
            let sid=next_nonempty_shelf(&self.shelves,shelf_id,direction);
            if sid==shelf_id { continue; }
            let shelf=&self.shelves[sid];
            for delta in [-1isize,1] {
                let index=(shelf.index as isize+delta)
                    .rem_euclid(shelf.carts.len().max(1) as isize) as usize;
                if !priority.contains(&(sid,index)) {
                    priority.push((sid,index));
                }
            }
        }
        for (sid,shelf) in self.shelves.iter().enumerate() {
            if !shelf.carts.is_empty() && !priority.contains(&(sid,shelf.index)) {
                priority.push((sid,shelf.index));
            }
        }
        if quiet {
            // Fill the 160-entry LRU while idle, not while the D-pad is
            // moving. Active system first; other systems around their focus.
            for (sid,shelf) in self.shelves.iter().enumerate() {
                let radius=if sid==shelf_id {80} else {16};
                for index in shelf.around(radius) {
                    if priority.len()>=CART_IDLE_PREFETCH_CAP {break}
                    if !priority.contains(&(sid,index)) {priority.push((sid,index));}
                }
                if priority.len()>=CART_IDLE_PREFETCH_CAP {break}
            }
        }
        self.cart_render.prioritize(&priority);
        for (sid, index) in priority {
            // Clear-cart composite is expensive: delay its three material
            // uploads until the carousel has settled.
            let wants_material=quiet && sid==self.active && index==selected &&
                self.shelves[sid].carts.get(index).is_some_and(slot_ui::is_clear) &&
                self.clear_material.map(|m|m.key)!=Some((sid,index));
            if self.shelves[sid].face(index).is_some() {
                if let Some(pos) = self.texture_cache.iter()
                    .position(|(s, i, _)| *s == sid && *i == index)
                {
                    if let Some(entry) = self.texture_cache.remove(pos) {
                        self.texture_cache.push_back(entry);
                    }
                }
                if !wants_material {continue;}
            }
            // A freshly configured AVD has no ROMs yet. Skip the empty
            // shelf instead of indexing carts[0] and aborting the GL thread.
            let Some(cart) = self.shelves.get(sid).and_then(|shelf| shelf.carts.get(index))
            else { continue; };
            self.cart_render.request((sid, index), cart, wants_material);
        }
    }

    fn ensure_ra_service(&mut self) {
        if self.ra_signing_out || self.ra_service.is_some() { return; }
        let root = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .as_ref().map(|(root, _)| root.clone());
        if let Some(root) = root {
            // Explicit opt-in from the Android account login. Never start RA
            // until an actual private config exists.
            if root.join("Config/retroachievements.toml").is_file() {
                self.ra_service = Some(slot_achievements::Service::start(root));
            }
        }
    }

    fn prepare_game_core(&mut self, local: &str) {
        let paths = PATHS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some((storage, library)) = paths {
            let uri = self.requested_uri.as_deref().unwrap_or_default();
            let is_gba = self.shelves[self.active].carts
                .get(self.shelves[self.active].index)
                .is_some_and(|c| c.platform == Platform::Gba);
            let core = if is_gba {
                core_selection::selected(&storage, uri)
            } else {
                Core::Mgba
            };
            let core_file = library.join(format!("lib{}_libretro.so", core.as_str()));
            let platform = self.shelves[self.active].carts
                .get(self.shelves[self.active].index)
                .map_or(Platform::Gba, |cart|cart.platform);
            // First game still wakes RA if the asynchronous SAF scan has not run.
            // Perform this mutable operation BEFORE borrowing rom_stem.
            // GBA, GB and GBC use the verified per-console memory maps.
            self.ensure_ra_service();
            let rom_stem = self.shelves[self.active].carts
                .get(self.shelves[self.active].index)
                .map_or("", |cart| cart.stem.as_str());
            let effective_settings = self.game_settings();
            let started = Instant::now();
            let resume_preference = NEXT_RESUME_MODE.swap(-1, Ordering::AcqRel);
            let wants_state = match resume_preference {
                0 => false, 1 => true,
                _ => effective_settings.eject_save && !self.fresh_launch,
            };
            match GameSession::open(Path::new(local), &core_file, &storage,
                core, platform, wants_state,
                rom_stem, effective_settings.gb_palettes.then(|| effective_settings.palette_name()),
                effective_settings.shader_for(platform)
                    .core_colour(effective_settings.colour_for(platform)),
                self.ra_service.as_ref()) {
                Ok(session) => {
                    self.link_identity = if platform == Platform::Gba {
                        crate::link_rom::identify(Path::new(local)).ok()
                    } else { None };
                    self.link_mode_override = None;
                    // Create libretro on the same GL thread, but defer audio
                    // and CRT presentation until Slot's seated hold completes.
                    crate::game::log_launch_timing(&format!(
                        "Core prepared: {}ms loading, {}ms since A",
                        started.elapsed().as_millis(),
                        self.insert_started.map_or(0, |at| at.elapsed().as_millis())
                    ));
                    self.pending_game = Some(session);
                }
                Err(error) => {
                    set_message(format!("{}: {error}", core.text()));
                    self.inserted = false;
                    self.requested_uri = None;
                }
            }
        } else {
            set_message("Android game paths are not configured".into());
            self.inserted = false;
            self.requested_uri = None;
        }
    }

    fn add_text(&self, index: usize, x: f32, y: f32, out: &mut Vec<Draw>) {
        self.add_text_alpha(index, x, y, 1.0, out);
    }

    fn add_text_alpha(&self, index: usize, x: f32, y: f32, alpha: f32, out: &mut Vec<Draw>) {
        let (tex, w, h) = self.menu_textures[index];
        out.push(Draw::Tex { x, y, w: w as f32, h: h as f32, tex, alpha });
    }

    fn text_fit(&self, index: usize, x: f32, y: f32, max_w: f32,
                alpha: f32, out: &mut Vec<Draw>) {
        let (tex, raw_w, raw_h) = self.menu_textures[index];
        // Keep original Slot typography, but fit long settings labels without
        // horizontally stretching them or overflowing a 3:2 display.
        if raw_w == 0 || raw_h == 0 { return; }
        let scale = 0.79f32.min(max_w / raw_w as f32);
        out.push(Draw::Tex {
            x, y, w: raw_w as f32 * scale, h: raw_h as f32 * scale,
            tex, alpha,
        });
    }

    fn modal(&self, x: f32, y: f32, w: f32, h: f32,
             out: &mut Vec<Draw>, dim: f32) {
        out.push(Draw::Rect {
            x: 0.0, y: 0.0, w: OUT_W as f32, h: OUT_H as f32,
            colour: [0.0, 0.0, 0.0, dim],
        });
        out.push(Draw::Rect {
            x, y, w, h, colour: [0.085, 0.085, 0.093, 1.0],
        });
        out.push(Draw::Rect {
            x, y, w, h: 2.0, colour: [0.69, 0.69, 0.72, 1.0],
        });
    }

    fn draw_menu_hints(&self, x: f32, y: f32, out: &mut Vec<Draw>) {
        let mut x = x;
        for (tex, w) in self.menu_hints {
            out.push(Draw::Tex { x, y, w: w as f32, h: HINT_H as f32,
                tex, alpha: 1.0 });
            x += w as f32 + 24.0;
        }
    }

    fn prepare_about(&mut self) {
        if self.about_texture.is_none() {
            let sticker = sticker_face_konkr(&StickerFields {
                battery: self.battery_percent, serial: "0000130", dirty_digit: '0',
            });
            self.about_texture = Some(self.gpu.create_texture(
                sticker.w, sticker.h, &sticker.rgba,
            ));
        }
    }

    fn current_cart_platform(&self) -> Platform {
        self.shelves[self.active].carts
            .get(self.shelves[self.active].index)
            .map_or(Platform::Gba, |cart| cart.platform)
    }

    fn apply_video_geometry(&mut self) {
        let platform = self.current_cart_platform();
        let (w, h) = platform.picture();
        let x = (slot_gfx::SRC_W - w) as f32 / (2 * slot_gfx::SRC_W) as f32;
        let y = (slot_gfx::SRC_H - h) as f32 / (2 * slot_gfx::SRC_H) as f32;
        let x2 = 1.0 - x;
        let y2 = 1.0 - y;
        self.gpu.set_picture([x, y, x2, y2]);
        self.gpu.set_game_source_rect(video_mode::source_rect(platform, self.video_mode));
    }

    fn set_video_mode(&mut self, mode: VideoMode) {
        if self.video_mode == mode || self.current_cart_platform() == Platform::Gba {
            return;
        }
        self.video_mode = mode;
        self.apply_video_geometry();
        let name = self.shelves[self.active].carts
            .get(self.shelves[self.active].index)
            .map_or("", |cart| cart.stem.as_str());
        if let Some((root, _)) = PATHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if let Err(err) = video_mode::write_video_mode(root, name, mode) {
                set_message(format!("Cannot save display mode: {err}"));
            }
        }
    }

    /// Merge this ROM+core's optional visual/gameplay overrides on top of
    /// globally persisted Settings without mutating them or another ROM.
    fn game_settings(&self) -> crate::settings::Settings {
        let mut effective = self.settings.clone();
        if let Some(profile) = self.game_emulation {
            profile.apply_visual_and_gameplay(&mut effective, self.current_cart_platform());
        }
        effective
    }
    fn live_game_options(&self) -> LiveGameOptions {
        LiveGameOptions::from_profile(&self.settings, self.game_emulation)
    }
    fn game_shader(&self) -> crate::settings::Shader {
        self.game_settings().shader_for(self.current_cart_platform())
    }

    /// Native 3:2 KONKR: GBA fills the panel and has no forced letterbox bezel.
    /// GB/GBC preserve the official 720×480 transparent overlays.
    fn game_bezel(&self) -> Option<slot_gfx::TexId> {
        if self.game_settings().bezel_for(self.current_cart_platform())
            == crate::settings::Bezel::None {return None;}
        match self.current_cart_platform() {
            Platform::Gba=>None,
            Platform::Gb=>Some(self.gb_bezel_tex),
            Platform::Gbc=>Some(self.gbc_bezel_tex),
        }
    }

    fn screen_rows(platform: Platform)-> &'static [usize] {
        match platform {
            // Upstream v1.7 hides GBA bezel on an unframed 3:2 panel:
            // GBA already fills KONKR's native 960x640 display.
            Platform::Gba=>&[34,29],
            Platform::Gbc=>&[88,29,90],
            Platform::Gb=>&[89,29,36,90],
        }
    }
    fn screen_keys(platform:Platform)-> &'static [usize] {
        match platform {
            Platform::Gba=>&[0,22],
            Platform::Gbc=>&[21,23,26],
            Platform::Gb=>&[1,24,3,27],
        }
    }
    fn screen_title(platform:Platform)->usize {
        match platform {
            Platform::Gba=>91, Platform::Gbc=>92,Platform::Gb=>93,
        }
    }
    fn screen_submenu_platform(row:usize)->Platform {
        match row {
            0=>Platform::Gba,1=>Platform::Gbc,_=>Platform::Gb,
        }
    }

    fn pause_screen_rows(&self) -> &'static [usize] {
        if self.current_cart_platform()==Platform::Gb &&
            !self.game.as_ref().is_some_and(GameSession::can_change_gb_palette) {
            &[89,29,90]
        } else {
            Self::screen_rows(self.current_cart_platform())
        }
    }
    fn pause_screen_key(&self,row:usize)->Option<usize> {
        if self.current_cart_platform()==Platform::Gb &&
            !self.game.as_ref().is_some_and(GameSession::can_change_gb_palette) {
            [1,24,27].get(row).copied()
        } else {
            Self::screen_keys(self.current_cart_platform()).get(row).copied()
        }
    }
    fn pause_settings_rows(&self) -> &'static [usize] {
        if self.game.as_ref().is_some_and(GameSession::linked) {
            // Display-only settings cannot alter GBA Link/RFU state.
            &[32]
        } else {
            &[32,33,49,57]
        }
    }

    fn current_emulation(&self) -> EmulationProfile {
        let mut profile = self.game_emulation
            .unwrap_or_else(|| EmulationProfile::from_settings(&self.settings));
        // Legacy per-cart profiles inherit global Show Stats. Newly
        // customized profiles can override it explicitly per cartridge.
        profile.show_fps = profile.show_fps_override.unwrap_or(self.settings.show_fps);
        // Only gpSP needs the private mixer snapshot ABI. If an unexpected
        // unpatched core is loaded, do not run speculative frames with audio
        // corruption; preserve the user's requested profile on disk.
        if self.game.as_ref().is_some_and(|game| !game.runahead_supported()) {
            profile.run_ahead_frames = 0;
        }
        profile
    }
    fn selected_emulation_core(&self) -> Core {
        // GB/GBC never invoke gpSP, even if a stale core preference exists.
        if self.current_cart_platform() != Platform::Gba { return Core::Mgba; }
        let Some(uri) = self.selected_rom_uri() else { return Core::Mgba; };
        PATHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
            .map(|(root, _)| core_selection::selected(root, &uri))
            .unwrap_or(Core::Mgba)
    }
    fn selected_rom_uri(&self) -> Option<String> {
        self.shelves.get(self.active)?.carts
            .get(self.shelves[self.active].index)
            .map(|cart| cart.rom.to_string_lossy().into_owned())
    }
    fn load_selected_emulation(&mut self) {
        let uri = self.selected_rom_uri();
        let core = self.selected_emulation_core();
        self.game_emulation = uri.as_deref().and_then(|uri| {
            PATHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                .and_then(|(root, _)| EmulationProfile::load(root, uri, core))
        });
    }
    fn change_game_screen_or_gameplay(&mut self, key: usize, right: bool) {
        let Some(uri)=self.selected_rom_uri() else { return };
        let core=self.selected_emulation_core();
        let platform=self.current_cart_platform();
        let linked=self.game.as_ref().is_some_and(GameSession::linked);
        let relaunch=self.game.as_ref().is_some_and(GameSession::requires_relaunch);
        // Shader and color-correction are display-only and safe while linked;
        // never change input, save states, emulation speed or rewind in Link.
        if relaunch || (linked && !matches!(key,0|1|2|21|22|23|24|25|26|27)) {
            set_message("Gameplay changes are disabled during Multiplayer".into());
            return;
        }
        // This method is only called from the paused game, not the carousel.
        let mut profile=self.game_emulation
            .unwrap_or_else(|| EmulationProfile::from_settings(&self.settings));
        if !profile.change_visual_and_gameplay(&self.settings,platform,key,right) { return; }
        if let Some((root,_))=PATHS.lock().unwrap_or_else(|e|e.into_inner()).as_ref() {
            if let Err(error)=profile.save(root,&uri,core) {
                set_message(format!("Cannot save game settings: {error}"));
                return;
            }
        } else { return; }
        self.game_emulation=Some(profile);
        let view=self.game_settings();
        if matches!(key,0|1|21) {
            self.gpu.set_screen_effect(view.shader_for(platform).effect());
        }
        // A shader change can switch to/from a preset with its own GBA Color.
        // Recompute core colour here as well as when the correction toggle moves.
        if matches!(key,0|1|21|2|22|23|24) {
            if let Some(session)=self.game.as_mut() {
                session.set_colour_correction(view.shader_for(platform)
                    .core_colour(view.colour_for(platform)));
            }
        }
        if key==3 {
            if let Some(session)=self.game.as_mut() {
                session.set_gb_palette_mode(view.gb_palettes,view.palette_name());
            }
        }
        if key==5 {
            FF_AUDIO_ENABLED.store(view.ff_sound,Ordering::Release);
            AUDIO.lock().unwrap_or_else(|e|e.into_inner()).clear();
        }
        // No stale FF, rewind or frame debt after an option changes.
        if (4..=9).contains(&key) { self.reset_time_controls(); }
    }

    fn change_game_emulation(&mut self, key: usize, right: bool) {
        if self.game.as_ref().is_some_and(|game|game.linked() || game.requires_relaunch()) {
            // In a linked session, do not alter the effective frame cadence
            // or speculative time travel. Keep the current session in sync.
            set_message("Emulation settings are locked while Multiplayer is active".into());
            return;
        }
        if key == 12 && self.game.as_ref().is_some_and(|game|
            !game.runahead_supported()) {
            set_message("Run-Ahead unavailable for this core/game (Link, Flash or speculative audio protection)".into());
            return;
        }
        if key == 12 && self.game.as_ref().is_some_and(|game|
            game.mgba_flash_runahead_blocked()) {
            set_message("Run-Ahead is disabled for Pokemon Gen III with mGBA to protect Flash saves. gpSP is unchanged.".into());
            return;
        }
        let Some(uri) = self.selected_rom_uri() else { return; };
        let core = self.selected_emulation_core();
        // Edit the persisted intent, not the safety-clamped effective profile.
        // A core that temporarily disables speculative frames must not erase
        // saved Run-Ahead preference when we change Show Stats / pacing.
        let mut profile = self.game_emulation
            .unwrap_or_else(|| EmulationProfile::from_settings(&self.settings));
        profile.show_fps = profile.show_fps_override.unwrap_or(self.settings.show_fps);
        if !profile.change(key, right) { return; }
        if let Some((root, _)) = PATHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if let Err(error) = profile.save(root, &uri, core) {
                set_message(format!("Cannot save game emulation settings: {error}"));
                return;
            }
        } else { return; }
        self.game_emulation = Some(profile);
        // Avoid reinterpreting a frame debt accumulated under the previous
        // timing policy, and discard any prior predictive video/audio frame.
        // The core stays alive; edits take effect on the first resumed frame.
        self.game_accum = 0.0;
        AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    fn reset_game_emulation(&mut self) {
        if self.game.as_ref().is_some_and(|game|game.linked() || game.requires_relaunch()) {
            set_message("Cannot reset emulation during Multiplayer".into());
            return;
        }
        let Some(uri) = self.selected_rom_uri() else { return; };
        let core = self.selected_emulation_core();
        if let Some((root, _)) = PATHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if let Err(error) = EmulationProfile::clear(root, &uri, core) {
                set_message(format!("Cannot reset game emulation settings: {error}"));
                return;
            }
        } else { return; }
        self.game_emulation = None;
        let globals=self.game_settings();
        let platform=self.current_cart_platform();
        self.gpu.set_screen_effect(self.game_shader().effect());
        if let Some(session)=self.game.as_mut() {
            session.set_colour_correction(globals.shader_for(platform)
                .core_colour(globals.colour_for(platform)));
            session.set_gb_palette_mode(globals.gb_palettes,globals.palette_name());
        }
        FF_AUDIO_ENABLED.store(globals.ff_sound,Ordering::Release);
        self.reset_time_controls();
        AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    fn persist_settings(&self) {
        if let Some((root, _)) = PATHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if let Err(error) = self.settings.save(root) {
                set_message(format!("Cannot save Settings: {error}"));
            }
        }
    }

    fn reload_wallpaper(&mut self, root: Option<&Path>) {
        let (image, personal) = wallpaper_pixels(root, &self.settings.wallpaper);
        self.wallpaper_personal = personal;
        if let Some(rgba) = image {
            self.wallpaper_texture = Some(if let Some(tex) = self.wallpaper_texture {
                self.gpu.update_texture(tex, slot_ui::NATIVE_W, slot_ui::NATIVE_H, &rgba);
                tex
            } else {
                self.gpu.create_texture(slot_ui::NATIVE_W, slot_ui::NATIVE_H, &rgba)
            });
        }
        // None deliberately leaves the texture allocated for quick reactivation
        // but never draws it, avoiding repeated GL allocations on toggling.
    }

    fn refresh_wallpaper_name(&mut self) {
        let crate::settings::WallpaperPick::File(_) = &self.settings.wallpaper
            else { return };
        let name = self.settings.wallpaper.label();
        let short: String = name.chars().take(18).collect();
        let face = quick_value_face(&short, true);
        self.wallpaper_name_face = Some(
            if let Some((tex, _, _)) = self.wallpaper_name_face {
                self.gpu.update_texture(tex, face.w, face.h, &face.rgba);
                (tex, face.w, face.h)
            } else {
                (self.gpu.create_texture(face.w, face.h, &face.rgba), face.w, face.h)
            });
    }

    fn cycle_wallpaper(&mut self, right: bool) {
        let root = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .as_ref().map(|(root,_)|root.clone());
        let Some(root) = root.as_deref() else { return };
        self.settings.wallpaper = self.settings.wallpaper.step(root, right);
        self.reload_wallpaper(Some(root));
        self.refresh_wallpaper_name();
        self.persist_settings();
    }

    fn recents_root() -> Option<PathBuf> {
        PATHS.lock().unwrap_or_else(|e|e.into_inner())
            .as_ref().map(|(root,_)|root.clone())
    }
    fn persist_focus_if_idle(&mut self) {
        let current=self.shelves.get(self.active)
            .and_then(|shelf|shelf.carts.get(shelf.index))
            .map(|cart|(self.active,cart.rom.to_string_lossy().into_owned()));
        if current!=self.focus_seen {
            self.focus_seen=current;
            self.focus_dirty_since=Some(Instant::now());
        }
        if self.focus_dirty_since.is_some_and(|since|since.elapsed().as_millis()>=500) {
            self.focus_dirty_since=None;
            if let Some((rack,uri))=self.focus_seen.as_ref() {
                if let Some(root)=Self::recents_root() {
                    let focus=crate::selection::Focus { rack:*rack,uri:uri.clone() };
                    if let Err(err)=crate::selection::store(&root,&focus) {
                        eprintln!("slot-konkr: last selection could not persist: {err}");
                    }
                }
            }
        }
    }
    fn toggle_favorite(&mut self) {
        let Some(uri)=self.selected_rom_uri() else {return};
        let Some(root)=Self::recents_root() else {return};
        match crate::favorites::toggle(&root,&uri) {
            Ok(_) => self.rebuild_favorites(),
            Err(error)=>set_message(format!("Could not save favorite: {error}")),
        }
    }
    fn rebuild_favorites(&mut self) {
        let Some(root)=Self::recents_root() else {return};
        let ids=crate::favorites::load(&root);
        let carts=crate::favorites::carts(&ids,
            &[&self.shelves[0].carts,&self.shelves[1].carts,&self.shelves[2].carts]);
        let was_favorite=self.active==4;
        let previous_index=self.shelves[4].index;
        let previous_uri=self.shelves[4].carts.get(previous_index).map(|c|c.rom.clone());
        self.cart_render.invalidate_shelf(4);
        let moves=self.shelves[4].replace_carts_preserving_visuals(carts);
        self.rekey_rack_visuals(4,&moves);
        // Unstarring from Favorites selects the next cart, exactly as upstream.
        if was_favorite && !self.shelves[4].carts.is_empty() &&
            previous_uri.as_ref().is_some_and(|uri|
                !self.shelves[4].carts.iter().any(|cart|&cart.rom==uri)) {
            let last=self.shelves[4].carts.len()-1;
            self.shelves[4].select(previous_index.min(last));
        }
        for shelf in &mut self.shelves {
            shelf.mark_starred(|cart|ids.iter()
                .any(|uri|uri==cart.rom.to_string_lossy().as_ref()));
        }
        if was_favorite && self.shelves[4].carts.is_empty() {
            self.active=next_nonempty_shelf(&self.shelves,0,1);
            self.platform_shown_at=Some(Instant::now());
        }
    }
    // Preserve cart textures/material across Recents or Favorites reorder.
    fn rekey_rack_visuals(&mut self, rack:usize, moves:&[(usize,usize)]) {
        for (shelf,index,_) in &mut self.texture_cache {
            if *shelf!=rack {continue;}
            if let Some((_,new))=moves.iter().find(|(old,_)|*old==*index) {
                *index=*new;
            } else {
                *shelf=usize::MAX;
                *index=usize::MAX;
            }
        }
        if let Some(material)=self.clear_material.as_mut() {
            if material.key.0==rack {
                if let Some((_,new))=moves.iter().find(|(old,_)|*old==material.key.1) {
                    material.key.1=*new;
                } else {
                    material.key=(usize::MAX,usize::MAX);
                }
            }
        }
    }
    fn rebuild_recents(&mut self) {
        let Some(root)=Self::recents_root() else { return };
        let ids=crate::recents::load(&root);
        let carts=crate::recents::carts(&ids,
            &[&self.shelves[0].carts,&self.shelves[1].carts,&self.shelves[2].carts]);
        let was_recent=self.active==3;
        // Reject completed worker art from the previous URI->index mapping.
        self.cart_render.invalidate_shelf(3);
        let moves=self.shelves[3].replace_carts_preserving_visuals(
            if self.settings.show_recents { carts } else { Vec::new() });
        self.rekey_rack_visuals(3,&moves);
        // Restocking Recents must not erase Favorite stars: its Shelf rebuild
        // necessarily reconstructs the per-cart starred flags from scratch.
        let favorites=crate::favorites::load(&root);
        self.shelves[3].mark_starred(|cart|favorites.iter()
            .any(|uri|uri==cart.rom.to_string_lossy().as_ref()));
        if was_recent && self.shelves[3].carts.is_empty() {
            self.active=next_nonempty_shelf(&self.shelves,0,1);
        }
    }
    fn record_recent(&mut self) {
        let Some(root)=Self::recents_root() else { return };
        let Some(uri)=self.selected_rom_uri() else { return };
        // Launches may immediately leave the shelf renderer. Flush the selected
        // game identity now, rather than relying on a 500ms idle debounce.
        let focus=crate::selection::Focus {rack:self.active,uri:uri.clone()};
        if let Err(err)=crate::selection::store(&root,&focus) {
            eprintln!("slot-konkr: launch shelf-focus save failed: {err}");
        } else {
            self.focus_seen=Some((focus.rack,focus.uri));
            self.focus_dirty_since=None;
        }
        let next=crate::recents::append_previous(&crate::recents::load(&root),&uri);
        if let Err(err)=crate::recents::store(&root,&next) {
            eprintln!("slot-konkr: unable to update recents rack: {err}");
        } else {
            self.rebuild_recents();
        }
    }
    fn general_rows(&self) -> Vec<usize> {
        let mut rows=vec![83]; // Wallpaper
        if self.show_general_tint(){rows.push(84);}
        rows.extend_from_slice(&[85,86,87,43]);
        if self.custom_wallpaper_present {rows.push(45);}
        rows
    }
    fn cycle_general_row(&mut self, row: usize, right: bool, confirm: bool) {
        let Some(which)=self.general_rows().get(row).copied() else {return};
        match which {
            83 => self.cycle_wallpaper(right),
            84 => self.cycle_wallpaper_tint(right),
            85 => self.cycle_appearance(),
            86 => {
                self.settings.show_recents=!self.settings.show_recents;
                self.rebuild_recents();
                self.persist_settings();
            }
            87 => {
                self.settings.show_titles=!self.settings.show_titles;
                self.selected_title_name.clear();
                self.persist_settings();
            }
            43 if confirm => UI_ACTION.lock().unwrap_or_else(|e|e.into_inner()).push_back(8),
            45 if confirm => {
                UI_ACTION.lock().unwrap_or_else(|e|e.into_inner()).push_back(10);
                self.custom_wallpaper_present=false;
                self.overlay=ShelfOverlay::GeneralSettings {
                    row: row.saturating_sub(1),
                };
            }
            _ => {}
        }
    }
    /// Rasterise only when the selected game changes; no repeated label
    /// processing on 60/120Hz draw callbacks.
    fn show_selected_letter(&mut self) {
        let Some(cart)=self.shelves.get(self.active)
            .and_then(|shelf|shelf.carts.get(shelf.index)) else {return};
        let letter=slot_store::initial(&cart.stem);
        let face=slot_ui::word_ink(&letter.to_string(),[255,255,255]);
        let tex=match self.letter_texture {
            Some(tex)=>{self.gpu.update_texture(tex,face.w,face.h,&face.rgba);tex},
            None=>{let tex=self.gpu.create_texture(face.w,face.h,&face.rgba);
                self.letter_texture=Some(tex);tex},
        };
        self.letter_face=Printed::new(tex,face.w);
        // Upstream clears the previous system-name display when a letter jump
        // occurs. Otherwise a quick UP just after L1/R1 stays invisible.
        self.platform_shown_at=None;
        self.letter_shown_at=Some(Instant::now());
    }

    fn refresh_selected_title(&mut self) {
        if !self.settings.show_titles || self.game.is_some() {return;}
        // Original v1.7 shelf Game Titles use label_text() (cleaned
        // filename), NOT the GBA internal ROM header. Reuse the same value
        // and Slot's native fit/ellipsis, without arbitrary 39-char clipping.
        let name=self.shelves.get(self.active)
            .and_then(|s|s.carts.get(s.index))
            .map(slot_ui::label_text).unwrap_or_default();
        if name==self.selected_title_name {return;}
        self.selected_title_name=name.clone();
        if name.is_empty(){self.selected_title_face=Printed::default();return;}
        let image=slot_ui::title_line_face(&name);
        if image.w==0 || image.h==0 {
            self.selected_title_face=Printed::default();
            return;
        }
        let tex=if let Some(tex)=self.selected_title_texture {
            self.gpu.update_texture(tex,image.w,image.h,&image.rgba);
            tex
        }else{
            let tex=self.gpu.create_texture(image.w,image.h,&image.rgba);
            self.selected_title_texture=Some(tex);
            tex
        };
        self.selected_title_face=Printed::new(tex,image.w);
    }

    fn show_general_tint(&self) -> bool {
        self.wallpaper_personal
    }

    fn cycle_wallpaper_tint(&mut self, right: bool) {
        self.settings.wallpaper_tint=self.settings.wallpaper_tint.step(right);
        self.persist_settings();
    }

    fn cycle_appearance(&mut self) {
        self.settings.appearance=self.settings.appearance.toggle();
        let custom_colours = PATHS.lock().unwrap_or_else(|e| e.into_inner())
            .as_ref().is_some_and(|(root,_)|
                root.join("Config/theme.txt").is_file());
        self.slot_faces=if custom_colours { None }
            else if self.settings.appearance == crate::settings::Appearance::Light {
                Some(self.slot_light_faces)
            } else { Some(self.slot_dark_faces) };
        self.recolour_footer_text();
        self.persist_settings();
    }

    fn change_setting(&mut self, key: usize, right: bool) {
        if !self.settings.change(key, right) { return; }
        if key == 5 {
            FF_AUDIO_ENABLED.store(self.settings.ff_sound, Ordering::Release);
            AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
        if matches!(key,0|1|21|2|22|23|24) {
            let platform=self.current_cart_platform();
            let colour=self.settings.shader_for(platform)
                .core_colour(self.settings.colour_for(platform));
            if let Some(game) = self.game.as_mut() {
                game.set_colour_correction(colour);
            }
        }
        self.persist_settings();
    }

    fn setting_value(&self, key: usize) -> usize {
        use crate::settings::Shader;
        let profile = if matches!(self.overlay, ShelfOverlay::PauseEmulationSettings { .. }) {
            self.current_emulation()
        } else { EmulationProfile::from_settings(&self.settings) };
        let scoped = if matches!(self.overlay,
            ShelfOverlay::PauseScreenSettings { .. } |
            ShelfOverlay::PauseGameplaySettings { .. }) {
            self.game_settings()
        } else { self.settings.clone() };
        let shader = |shader| match shader {
            Shader::Off => 5, Shader::Lcd3x => 6, Shader::Grid => 7,
            Shader::Dot => 8, Shader::Simpletex => 9,
             Shader::LcdGridV2 => 27, Shader::LcdGridV2Gba => 28,
             Shader::ZfastLcd => 29, Shader::Lcd1x => 30,
             Shader::SimpletexGba => 31, Shader::Agb001 => 32,
             Shader::Ags001 => 33, Shader::SharpBilinear => 34,
        };
        match key {
            0 => shader(scoped.shader_gba),
            1 => shader(scoped.shader_gb),
            2 => if scoped.colour_for(self.current_cart_platform()) {4}else{5},
            3 => if scoped.gb_palettes { 4 } else { 5 },
            4 => match scoped.ff_speed { 2 => 0, 3 => 1, 4 => 2, _ => 3 },
            5 => if scoped.ff_sound { 4 } else { 5 },
            6 => if scoped.rewind { 4 } else { 5 },
            7 => if scoped.turbo { 15 } else { 16 },
            8 => if scoped.rumble { 4 } else { 5 },
            9 => if scoped.eject_save { 4 } else { 5 },
            10 => if profile.show_fps { 4 } else { 5 },
            11 => if profile.display_sync_pacing { 14 }
                else if profile.responsive_pacing { 11 } else { 10 },
            12 => match profile.run_ahead_frames { 1 => 12, 2 => 13, _ => 5 },
            13 => if self.settings.show_wifi { 4 } else { 5 },
            14 => if self.settings.show_bluetooth { 4 } else { 5 },
            15 => if self.settings.show_integrations { 4 } else { 5 },
            16 => match &self.settings.wallpaper {
                crate::settings::WallpaperPick::None => 17,
                crate::settings::WallpaperPick::Random => 18,
                crate::settings::WallpaperPick::Default => 19,
                crate::settings::WallpaperPick::Blur => 20,
                crate::settings::WallpaperPick::File(_) => 26,
            },
            17 => match self.settings.wallpaper_tint {
                crate::settings::WallpaperTint::Off => 5,
                crate::settings::WallpaperTint::Low => 23,
                crate::settings::WallpaperTint::Medium => 24,
                crate::settings::WallpaperTint::High => 25,
            },
            18 => if self.settings.appearance == crate::settings::Appearance::Light
                { 22 } else { 21 },
            19 => if self.settings.show_recents {4}else{5},
            20 => if self.settings.show_titles {4}else{5},
            21 => shader(scoped.shader_gbc),
            22 => if scoped.colour_correction_gba {4}else{5},
            23 => if scoped.colour_correction_gbc {4}else{5},
            24 => if scoped.colour_correction_gb {4}else{5},
            25 => if scoped.bezel_gba==crate::settings::Bezel::None {17}else{19},
            26 => if scoped.bezel_gbc==crate::settings::Bezel::None {17}else{19},
            27 => if scoped.bezel_gb==crate::settings::Bezel::None {17}else{19},
            _ => 5,
        }
    }

    /// Exact Settings ON/OFF texture and carets on Library row 4.
    fn draw_library_share_value(&self, row: usize, out: &mut Vec<Draw>) {
        let which = if SHARE_SAVES_ENABLED.load(Ordering::Acquire) { 4 } else { 5 };
        let (tex, width, height) = self.setting_values[which];
        let y = settings_row_top(6) + 4.0 * QUICK_PITCH + 8.0;
        let scale = 0.79f32;
        let w = width as f32 * scale;
        let right = OUT_W as f32 - 34.0;
        let selected = row == 4;
        let x = right - w - if selected { 22.0 } else { 0.0 };
        out.push(Draw::Tex { x, y, w, h: height as f32 * scale, tex,
            alpha: if selected { 1.0 } else { 0.62 } });
        if selected {
            let (left, lw, lh) = self.setting_carets[0];
            let (right_tex, rw, rh) = self.setting_carets[1];
            out.push(Draw::Tex { x: x - lw as f32 - 8.0, y,
                w: lw as f32, h: lh as f32, tex: left, alpha: 1.0 });
            out.push(Draw::Tex { x: right - rw as f32, y,
                w: rw as f32, h: rh as f32, tex: right_tex, alpha: 1.0 });
        }
    }

    fn draw_setting_values(&self, row: usize, keys: &[usize], out: &mut Vec<Draw>) {
        let top = settings_row_top(keys.len());
        for (index, key) in keys.iter().copied().enumerate() {
            if key > 27 { continue; } // Action-only rows have no value.
            let (tex, width, height) =
                if key == 16 && matches!(self.settings.wallpaper,
                    crate::settings::WallpaperPick::File(_)) {
                    self.wallpaper_name_face.unwrap_or(self.setting_values[26])
                } else {
                    self.setting_values[self.setting_value(key)]
                };
            let y = top + index as f32 * QUICK_PITCH + 8.0;
            let scale = 0.79;
            let w = width as f32 * scale;
            let right = OUT_W as f32 - 34.0;
            let chosen = row == index;
            let x = right - w - if chosen { 22.0 } else { 0.0 };
            out.push(Draw::Tex {
                x, y, w, h: height as f32 * scale,
                tex, alpha: if chosen { 1.0 } else { 0.62 },
            });
            if chosen {
                let (left, lw, lh) = self.setting_carets[0];
                let (right_tex, rw, rh) = self.setting_carets[1];
                out.push(Draw::Tex {
                    x: x - lw as f32 - 8.0, y, w: lw as f32,
                    h: lh as f32, tex: left, alpha: 1.0,
                });
                out.push(Draw::Tex {
                    x: right - rw as f32, y, w: rw as f32,
                    h: rh as f32, tex: right_tex, alpha: 1.0,
                });
            }
        }
        // The original Back / Change physical keycap legend.
        out.push(Draw::Rect {
            x: 0.0, y: 420.0, w: OUT_W as f32, h: 60.0,
            colour: opening(),
        });
        for (tex, w, x) in centred_hints(&self.settings_hints, LEGEND_GAP) {
            out.push(Draw::Tex {
                x, y: 427.0, w: w as f32, h: HINT_H as f32,
                tex, alpha: 1.0,
            });
        }
    }

    fn draw_original_style_menu(&self, heading: usize, items: &[usize],
                                selected: usize, out: &mut Vec<Draw>) {
        // The original Slot full-screen quick menu uses the housing palette,
        // full-width selected bar, 52px pitch, and centred physical keycaps.
        // Reuse those exact primitives for Android's library settings.
        out.push(Draw::Rect {
            x: 0.0, y: 0.0, w: OUT_W as f32, h: OUT_H as f32,
            colour: opening(),
        });
        let top = settings_row_top(items.len());
        let cursor = if self.menu_cursor_y.is_finite() { self.menu_cursor_y }
            else { top + selected as f32 * QUICK_PITCH };
        let intro = (self.menu_opened.elapsed().as_secs_f32() / 0.16).clamp(0.0, 1.0);
        let fade = slot_ui::ease(intro);
        out.push(Draw::Rect {
            x: 0.0, y: cursor + 4.0, w: OUT_W as f32,
            h: QUICK_PITCH - 8.0,
            colour: { let mut c = edge(); c[3] *= fade; c },
        });
        self.text_fit(heading, 32.0, 33.0, 650.0, 0.68 * fade, out);
        for (i, index) in items.iter().copied().enumerate() {
            let x = 37.0;
            let y = top + i as f32 * QUICK_PITCH + 8.0
                + (1.0 - fade) * 8.0;
            self.text_fit(index, x, y, 650.0, fade, out);
        }
        // All menus use the original Slot keycap generator. The paused game
        // screen additionally advertises its active SAVE / LOAD shortcuts.
        if heading == 19 {
            let linked = self.game.as_ref().is_some_and(|session|
                session.linked() || session.requires_relaunch());
            // A paused Link never offers save/load, even in its keycap hints.
            let hints: &[(slot_gfx::TexId, u32)] = if linked {
                &self.game_menu_hints[..1] // B Back only
            } else { &self.game_menu_hints };
            for (tex, w, x) in centred_hints(hints, 15.0) {
                out.push(Draw::Tex {
                    x, y: 427.0, w: w as f32, h: HINT_H as f32,
                    tex, alpha: fade,
                });
            }
        } else {
            for (tex, w, x) in centred_hints(&self.menu_hints, LEGEND_GAP) {
                out.push(Draw::Tex {
                    x, y: 427.0, w: w as f32, h: HINT_H as f32,
                    tex, alpha: fade,
                });
            }
        }
    }

    /// Exact upstream Slot Link menu composition: no additional Android
    /// title, mode label or bottom bar. Cable plug and wireless adapter sprites
    /// are identical to original and fork, as are progress/failure messages.
    fn draw_multiplayer(&self, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0, y: 0.0, w: OUT_W as f32, h: OUT_H as f32,
            colour: opening(),
        });
        link_screen::draw_link_art(self.link_phase, self.link_mode,
            self.link_millis(), &self.link_art, out);
        let text = match self.link_phase {
            LinkMenu::Pick(role) => Some(self.link_role_faces[role.index()]),
            LinkMenu::Working { step, .. } => Some(self.link_step_faces[step.index()]),
            LinkMenu::Linked { .. } => Some(self.link_connected_face),
            LinkMenu::Failed { fail, .. } => fail.shown()
                .and_then(|i| self.link_fail_faces.get(i).copied()),
            LinkMenu::Unplug { .. } => None,
        };
        if let Some((tex, w, h)) = text {
            out.push(Draw::Tex {
                x: link_heading_x(w),
                y: LINK_TEXT_Y, w: w as f32, h: h as f32,
                tex, alpha: 1.0,
            });
        }
        let hints: &[(slot_gfx::TexId, u32)] =
            match link_legend_profile(self.link_phase, self.link_mode_switchable()) {
                LinkLegendProfile::PickerWithMode => &self.link_switch_hints,
                LinkLegendProfile::Picker => &self.link_hints,
                LinkLegendProfile::Searching => &self.link_hints[..1],
                LinkLegendProfile::ConnectedMenu => &self.link_connected_hints,
                LinkLegendProfile::Failed => &self.link_failed_hints,
                LinkLegendProfile::None => &[],
            };
        let seen = |w: u32| w.saturating_sub(HINT_EDGE) as f32;
        let mut x = link_legend_start(hints);
        for &(tex, w) in hints {
            out.push(Draw::Tex {
                x, y: LINK_LEGEND_Y,
                w: w as f32, h: HINT_H as f32,
                tex, alpha: 1.0,
            });
            x += (seen(w) + LINK_LEGEND_GAP).round();
        }
    }

    fn draw_overlay(&self, out: &mut Vec<Draw>) {
        match self.overlay {
            ShelfOverlay::None => {}
            ShelfOverlay::Menu { row } => {
                self.draw_original_style_menu(0, &[1, 2, 3, 48, 25, 31], row, out);
            }
            ShelfOverlay::Library { row } => {
                self.draw_original_style_menu(5, &[6, 24, 7, 8, 79, 9], row, out);
                self.draw_library_share_value(row, out);
            }
            ShelfOverlay::Settings { row } => {
                self.draw_original_style_menu(26, &[81, 32, 33, 49, 58], row, out);
            }
            ShelfOverlay::GeneralSettings { row } => {
                let rows=self.general_rows();
                self.draw_original_style_menu(82,&rows,row,out);
                let keys:Vec<usize>=rows.iter().map(|id|match id {
                    83=>16,84=>17,85=>18,86=>19,87=>20,_=>usize::MAX
                }).collect();
                self.draw_setting_values(row,&keys,out);
            }
            ShelfOverlay::InterfaceSettings { row } => {
                self.draw_original_style_menu(59, &[63, 64, 65], row, out);
                self.draw_setting_values(row, &[13, 14, 15], out);
            }
            ShelfOverlay::EmulationSettings { row } => {
                self.draw_original_style_menu(50, &[51, 52, 55], row, out);
                self.draw_setting_values(row, &[10, 11, 12], out);
            }
            ShelfOverlay::PauseEmulationSettings { row } => {
                self.draw_original_style_menu(50, &[51, 52, 55], row, out);
                // Identify the selected core without changing Slot's menu
                // layout or existing keycap typography.
                let index = if self.selected_emulation_core() == Core::Gpsp { 1 } else { 0 };
                let (tex, w, h) = self.emulation_core_labels[index];
                // Match text_fit's 0.79x header scale and exactly the heading
                // baseline at y=33; right align to the same 32px inset.
                // Core names are short enough to never overlap the left title.
                let scale = 0.79f32;
                let fade = slot_ui::ease(
                    (self.menu_opened.elapsed().as_secs_f32() / 0.16).clamp(0.0, 1.0));
                out.push(Draw::Tex {
                    x: OUT_W as f32 - 32.0 - w as f32 * scale, y: 33.0,
                    w: w as f32 * scale, h: h as f32 * scale,
                    tex, alpha: 0.68 * fade,
                });
                // The pause menu has three visible rows, not four.
                // The extra action sentinel displaced ALL value glyphs by
                // half a row and desynchronised them from their carets.
                self.draw_setting_values(row, &[10, 11, 12], out);
            }
            ShelfOverlay::ScreenSettings { row } => {
                self.draw_original_style_menu(32, &[91,92,93], row, out);
            }
            ShelfOverlay::ScreenSystemSettings {platform,row} => {
                self.draw_original_style_menu(Self::screen_title(platform),
                    Self::screen_rows(platform),row,out);
                self.draw_setting_values(row,Self::screen_keys(platform),out);
            }
            ShelfOverlay::GameplaySettings { row } => {
                self.draw_original_style_menu(33, &[27, 28, 37, 38, 30, 39], row, out);
                self.draw_setting_values(row, &[4, 5, 6, 7, 8, 9], out);
            }
            ShelfOverlay::About => {
                out.push(Draw::Rect {
                    x: 0.0, y: 0.0, w: OUT_W as f32, h: OUT_H as f32,
                    colour: [0.055, 0.055, 0.065, 1.0],
                });
                if let Some(tex) = self.about_texture {
                    out.push(Draw::Tex {
                        x: (OUT_W - STICKER_W) as f32 / 2.0,
                        y: (OUT_H - STICKER_H) as f32 / 2.0,
                        w: STICKER_W as f32, h: STICKER_H as f32,
                        tex, alpha: 1.0,
                    });
                }
            }
            ShelfOverlay::Scraping | ShelfOverlay::Achievements => {
                self.modal(110.0, 127.0, 500.0, 227.0, out, 0.78);
                let (header, detail) = if matches!(self.overlay, ShelfOverlay::Scraping) {
                    (11, 12)
                } else {
                    (13, 14)
                };
                self.text_fit(header, 141.0, 152.0, 438.0, 1.0, out);
                self.text_fit(detail, 141.0, 230.0, 438.0, 0.65, out);
                self.draw_menu_hints(141.0, 311.0, out);
            }
            ShelfOverlay::GameMenu { row } => {
                let linked = self.game.as_ref().is_some_and(GameSession::linked);
                let relaunch = self.game.as_ref().is_some_and(GameSession::requires_relaunch);
                let indices: Vec<usize> = pause_rows(linked, relaunch).iter()
                    .map(|item| pause_row_text(*item, linked, relaunch)).collect();
                self.draw_original_style_menu(19, &indices, row, out);
            }
            ShelfOverlay::PauseSettings { row } => {
                self.draw_original_style_menu(26, self.pause_settings_rows(), row, out);
            }
            ShelfOverlay::PauseScreenSettings { row } => {
                let items=self.pause_screen_rows();
                self.draw_original_style_menu(32, items, row, out);
                let keys: Vec<usize>=(0..items.len())
                    .filter_map(|index|self.pause_screen_key(index)).collect();
                self.draw_setting_values(row,&keys,out);
            }
            ShelfOverlay::PauseGameplaySettings { row } => {
                self.draw_original_style_menu(33, &[27,28,37,38,30,39], row, out);
                self.draw_setting_values(row,&[4,5,6,7,8,9],out);
            }
            ShelfOverlay::Multiplayer => self.draw_multiplayer(out),
            ShelfOverlay::Core => {
                self.draw_original_core_picker(out);
            }
            ShelfOverlay::States => {
                // Rendered separately as full-screen original Slot Polaroids.
            }
        }
    }

    /// Slot's original board/lid/socket/chip geometry and timelines.
    /// The picker state machine is the unmodified upstream core_picker.rs.
    fn draw_original_core_picker(&self, out: &mut Vec<Draw>) {
        let Some(picker) = self.picker else { return };
        let now = self.born.elapsed().as_millis() as u64;
        let progress = picker.openness(now);
        let lift = lift_of(progress);
        let shelf = &self.shelves[self.active];
        let (rest, scale) = shelf.selected_at();
        let cart = shelf_cart_at(rest, scale);
        let board = board_from(cart, progress);
        let zoom = board_zoom(board);

        if let Some(tex) = self.board_texture {
            out.push(Draw::Tex {
                x: board.x, y: board.y, w: board.w, h: board.h,
                tex, alpha: 1.0,
            });
        }
        for (i, tex) in self.socket_textures.iter().copied().enumerate() {
            let (x, y) = on_board(board, SOCKET_U[i], SOCKET_V);
            out.push(Draw::Tex {
                x: x.round(), y: y.round(),
                w: SOCKET_W as f32 * zoom, h: SOCKET_H as f32 * zoom,
                tex, alpha: 1.0,
            });
        }

        let chip = picker.chip(now);
        let u = CHIP_U[0] + (CHIP_U[1] - CHIP_U[0]) * chip.across;
        if chip.lift > 0.0 {
            let (cx, cy) = on_board(board, u + 19.0, CHIP_V + 29.4);
            let (w, h) = (SHADOW_W as f32 * zoom, SHADOW_H as f32 * zoom);
            out.push(Draw::Tex {
                x: cx - w / 2.0, y: cy - h / 2.0, w, h,
                tex: self.chip_shadow, alpha: 0.6 * chip.lift * lift,
            });
        }
        let tex = chip.seated
            .map(|core| self.chip_textures[core.index()])
            .unwrap_or(self.chip_blank);
        let (x, y) = on_board(board, u, CHIP_V - HOP_LIFT * chip.lift);
        let body = grown(slot_ui::Placed {
            x: x + chip.shake, y,
            w: CHIP_W as f32 * zoom, h: CHIP_H as f32 * zoom,
        }, TURN_PAD as f32 * zoom);
        out.push(Draw::Turned {
            x: body.x.round(), y: body.y.round(),
            w: body.w, h: body.h, tex, alpha: 1.0, turn: chip.tip,
        });

        // The original lid slides up first (160ms), lifts and tilts (260ms)
        // and returns smoothly from any partial openness on B/A (320ms).
        let (lid, turn) = lid_from(cart, progress);
        let k = lid.w / slot_ui::lid_at(1.0).0.w;
        let (w, h) = (168.0 * k, 18.0 * k);
        out.push(Draw::Tex {
            x: lid.x + (lid.w - w) / 2.0,
            y: lid.y + lid.h + 29.0 * k - h / 2.0,
            w, h,
            tex: self.chip_shadow,
            alpha: 0.8 * lift,
        });
        let (behind,front)=self.clear_layers();
        let face=grown(lid,TURN_PAD as f32*lid.w/CART_W as f32);
        for tex in behind.into_iter().flatten()
            .chain(front.or_else(||shelf.face(shelf.index))) {
            out.push(Draw::Turned {
                x:face.x,y:face.y,w:face.w,h:face.h,
                tex,alpha:1.0,turn,
            });
        }

        // Restore the original Slot keycap legends. The original positions
        // anchor Cancel to the board's left edge, Swap in the centre and
        // Choose to the board's right edge. Shrink uniformly only if needed
        // to preserve >=12px spacing on small screens / font changes.
        let widths = self.core_legend_faces.map(|(_, w)| w.saturating_sub(HINT_EDGE));
        let positions = crate::core_legend::positions(widths);
        for ((tex, w), (x, scale)) in self.core_legend_faces.into_iter().zip(positions) {
            let height = HINT_H as f32 * scale;
            out.push(Draw::Tex {
                x: x.round(),
                y: 386.0 + (HINT_H as f32 - height) / 2.0,
                w: w as f32 * scale,
                h: height,
                tex,
                alpha: lift,
            });
        }
    }

    fn update_polaroid_title(&mut self) {
        let Some(p)=self.polaroids.as_mut() else {return;};
        let face=title_face(&p.title(&stamp_now()));
        let tex=match self.polaroid_title_texture {
            Some(tex)=>{
                self.gpu.update_texture(tex,face.w,face.h,&face.rgba);
                tex
            }
            None=>{
                let tex=self.gpu.create_texture(face.w,face.h,&face.rgba);
                self.polaroid_title_texture=Some(tex);
                tex
            }
        };
        p.set_title_face(Some(tex));
    }

    fn open_polaroids(&mut self) {
        let Some(game)=self.game.as_ref() else {return;};
        let entries=game.history();
        if entries.is_empty() {
            set_message("No saved states yet — SELECT + R1 to save".into());
            return;
        }
        let mut polaroids=Polaroids::new(entries);
        let mut faces=Vec::new();
        for (index,entry) in polaroids.entries.iter().enumerate() {
            let face=photo_face(entry);
            let tex=if let Some(tex)=self.photo_textures.get(index).copied() {
                self.gpu.update_texture(tex,face.w,face.h,&face.rgba);
                tex
            } else {
                let tex=self.gpu.create_texture(face.w,face.h,&face.rgba);
                self.photo_textures.push(tex);
                tex
            };
            faces.push(tex);
        }
        polaroids.set_faces(faces);
        let hints=[("B","Back"),("Y","Delete"),("A","Load"),("X","Undo")];
        let hint_faces=hints.map(|(key,label)| {
            let face=hint_face(key,label);
            self.gpu.create_texture(face.w,face.h,&face.rgba)
        });
        polaroids.set_hint_faces(hint_faces.to_vec());
        if game.undo_available() {polaroids.set_undo(Some("undo"));}
        self.polaroids=Some(polaroids);
        self.update_polaroid_title();
        self.overlay=ShelfOverlay::States;
    }

    // 400ms hold then the original accelerated 110/85/65/50ms repeat
    // in Slot-owned menus. Android key repeats are filtered at MainActivity.
    fn tick_menu_repeat(&mut self) {
        let Some((key,due,count))=self.menu_held else {return};
        if matches!(self.overlay,ShelfOverlay::None) {
            self.menu_held=None;
            return;
        }
        let now=self.born.elapsed().as_millis() as u64;
        if now<due {return;}
        const GAPS:[u64;4]=[110,85,65,50];
        self.menu_held=Some((key,now+GAPS[count.min(3)],count+1));
        self.menu_repeat_dispatch=true;
        self.handle(Input::Key(key,true));
        self.menu_repeat_dispatch=false;
    }
    fn draw(&mut self) {
        self.tick_menu_repeat();
        self.refresh_system_status();
        // Poll the already-running achievement evaluator without blocking GL.
        // Only Android's foreground UI displays notices; the worker never
        // touches JNI, an Activity, the renderer or the filesystem here.
        if let Some(service) = self.ra_service.as_ref() {
            let status = format!("{:?}", service.sync_status());
            let progress = service.sync_progress();
            let pending = service.sync_pending();
            let unsaved = service.has_unsaved_awards();
            let snapshot = serde_json::json!({
                "state": status,
                "pending": pending,
                "unsaved": unsaved,
                "progress": progress,
            }).to_string();
            *RA_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot);
            // Bound per-frame overhead and pending Android UI messages.
            for _ in 0..4 {
                let Some(notice) = service.take_notice() else { break };
                let payload = ra_notice_payload(
                    &notice.title, &notice.detail,
                    notice.kind == slot_achievements::NoticeKind::Earned,
                    notice.badge.as_deref(),
                );
                let mut queue = RA_NOTICES.lock().unwrap_or_else(|e| e.into_inner());
                queue_ra_notice(&mut queue, payload);
            }
        }
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.05);
        self.last = now;

        // FPS is a *real* libretro simulation measurement, not the display
        // refresh rate; the separate draw rate reveals missed vsyncs.
        if self.game.is_some() && self.current_emulation().show_fps {
            PERF_ACTIVE.store(true, Ordering::Release);
            self.perf_rendered += 1;
            let span = self.perf_last.elapsed().as_secs_f64();
            if span >= 1.0 {
                let emu = self.perf_emulated as f64 / span;
                let render = self.perf_rendered as f64 / span;
                let catchup = 100.0 * self.perf_catchup as f64 /
                    self.perf_rendered.max(1) as f64;
                let target = self.game.as_ref().map_or(60.0, |g| g.fps);
                *PERF_STATUS.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(format!("EMU {emu:.1}/{target:.1} | DISPLAY {render:.1} | CATCHUP {catchup:.0}%"));
                self.perf_last = now;
                self.perf_rendered = 0;
                self.perf_emulated = 0;
                self.perf_catchup = 0;
            }
        } else {
            self.perf_last = now;
            self.perf_rendered = 0;
            self.perf_emulated = 0;
            self.perf_catchup = 0;
            if PERF_ACTIVE.swap(false, Ordering::AcqRel) {
                *PERF_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }

        let events: Vec<Input> = INPUT
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .drain(..)
            .collect();
        for event in events {
            self.handle(event);
        }
        self.poll_multiplayer();
        if self.picker.is_some_and(|p| p.finished(self.born.elapsed().as_millis() as u64)) {
            self.picker = None;
            self.overlay = ShelfOverlay::None;
        }
        // Update full-screen menus and spring-smooth the selection bar.
        // Keep animation entirely in rendering: never delay controller input.
        let (kind, selected, count) = match self.overlay {
            ShelfOverlay::Menu { row } => (1u8, row, 6usize),
            ShelfOverlay::Library { row } => (2u8, row, 6usize),
            ShelfOverlay::GameMenu { row } => (3u8, row, pause_rows(
                self.game.as_ref().is_some_and(GameSession::linked),
                self.game.as_ref().is_some_and(GameSession::requires_relaunch)).len()),
            ShelfOverlay::PauseSettings { row } => (13u8, row, self.pause_settings_rows().len()),
            ShelfOverlay::PauseScreenSettings { row } =>
                (14u8, row, self.pause_screen_rows().len()),
            ShelfOverlay::PauseGameplaySettings { row } => (15u8, row, 6usize),
            ShelfOverlay::Multiplayer => (11u8, 0, 0usize),
            ShelfOverlay::Settings { row } => (4u8, row, 5usize),
            ShelfOverlay::GeneralSettings { row } =>
                (12u8, row, self.general_rows().len()),
            ShelfOverlay::InterfaceSettings { row } => (10u8, row, 3usize),
            ShelfOverlay::EmulationSettings { row } => (8u8, row, 3usize),
            ShelfOverlay::PauseEmulationSettings { row } => (9u8, row, 3usize),
            ShelfOverlay::ScreenSettings { row } => (5u8, row, 3usize),
            ShelfOverlay::ScreenSystemSettings { platform,row } =>
                (16u8,row,Self::screen_rows(platform).len()),
            ShelfOverlay::GameplaySettings { row } => (6u8, row, 6usize),
            _ => (0, 0, 0),
        };
        if kind != self.menu_seen {
            self.menu_seen = kind;
            self.menu_opened = now;
            if count > 0 {
                self.menu_cursor_y = settings_row_top(count)
                    + selected as f32 * QUICK_PITCH;
            }
        } else if count > 0 {
            let goal = settings_row_top(count) + selected as f32 * QUICK_PITCH;
            let response = 1.0 - (-dt * 24.0).exp();
            self.menu_cursor_y += (goal - self.menu_cursor_y) * response;
        }

        // Upstream CRT: 160ms extinction, 350ms black hold, 450ms ejection.
        // Do not move the cart during screen power-off or the blank hold.
        if self.exiting_screen || self.eject_hold_remaining > 0.0 {
            if self.exiting_screen {
                self.screen_power = (self.screen_power - dt / SCREEN_POWER_OFF_S).max(0.0);
            } else {
                self.eject_hold_remaining = (self.eject_hold_remaining - dt).max(0.0);
            }
            self.gpu.set_screen_power(self.screen_power);
            let mut commands = vec![Draw::Rect {
                x: 0.0, y: 0.0, w: OUT_W as f32, h: OUT_H as f32,
                colour: [0.085, 0.085, 0.093, 1.0],
            }];
            if let Some(shelf) = self.shelves.get(self.active) {
                if let Some(cart) = shelf.carts.get(shelf.index) {
                    let (rest, scale) = shelf.selected_at();
                    // Match upstream slot. v1.7: neighbours move out/fade in sync with
                    // the 450ms seat animation and return on reverse ejection.
                    shelf.draw_row(Some(cart.stem.as_str()), 0.0, self.progress, 1.0, &mut commands);
                    SlotChrome {
                        cart, face: shelf.face(shelf.index), rest, scale,
                        seat: self.progress, alert: None, dim: 0.0,
                        screen: self.screen_power, game: true, over: false,
                        slot_faces: self.slot_faces,
                    }.draw_with_material(&mut commands,self.clear_layers().0,self.clear_layers().1);
                } else {
                    commands.push(Draw::Game);
                }
            }
            self.gpu.fit(self.size);
            self.gpu.begin_frame();
            self.gpu.draw_list(&commands);
            self.gpu.end_frame(self.size);
            if self.screen_power <= 0.0 { self.exiting_screen = false; }
            return;
        }
        if self.game.is_some() {
            self.screen_power = (self.screen_power + dt / SCREEN_POWER_ON_S).min(1.0);
            self.gpu.set_screen_power(self.screen_power);
        }
        // Holding physical MENU saves, ejects, and returns to shelf without
        // requiring a second keypress or sending MENU to libretro.
        if self.game.is_some()
            && self.mode_down_at.is_some_and(|t| t.elapsed().as_millis() >= 650)
        {
            self.mode_down_at = None;
            self.handle(Input::Exit);
        }

        // A long A press skips loading the auto-save state, but preserves SRAM.
        // At least 400ms must pass before treating a held A as a fresh launch.
        if let Some(when) = self.a_down_at {
            if when.elapsed().as_millis() >= 400 {
                self.fresh_launch = true;
                self.a_down_at = None;
            }
        }
        // Mirror original Slot: the cartridge seats at 450ms and remains
        // seated until 730ms (or until the core is ready, if later).
        // Use the hold to initialize mGBA/gpSP and restore SRAM/auto-state
        // instead of running this work only AFTER the insertion has ended.
        if self.inserted && self.game.is_none() {
            // Read wall time, not accumulated clipped frame dt: a slow core
            // load must be hidden INSIDE the hold, not added after it.
            let inserted_for = self.insert_started.map_or(0.0, |t| t.elapsed().as_secs_f32());
            if inserted_for >= SEATED_AT && self.a_down_at.is_none()
                && self.pending_game.is_none()
            {
                if let Some(local) = self.prepared_rom.take() {
                    self.prepare_game_core(&local);
                }
            }
            // Re-read wall time after core.load() and save-state import.
            let inserted_for = self.insert_started.map_or(0.0, |t| t.elapsed().as_secs_f32());
            if inserted_for >= INSERT_S {
                if let Some(session) = self.pending_game.take() {
                    crate::game::log_launch_timing(&format!(
                        "Start CRT: {}ms since A (upstream target 730ms)",
                        self.insert_started.map_or(0, |at| at.elapsed().as_millis())
                    ));
                    self.buttons = 0;
                    SAMPLE_RATE.store(session.sample_rate, Ordering::Release);
                    self.game = Some(session);
                    self.record_recent();
                    self.game_accum = 0.0;
                    self.gpu.set_screen_effect(self.game_shader().effect());
                    // The exact Slot upstream geometry: GB/GBC 160x144
                    // is centred within the 240x160 libretro canvas.
                    let name = self.shelves[self.active].carts
                        .get(self.shelves[self.active].index)
                        .map_or("", |cart| cart.stem.as_str());
                    self.video_mode = PATHS.lock().unwrap_or_else(|e| e.into_inner())
                        .as_ref().map_or(VideoMode::Actual,
                            |(root, _)| video_mode::video_mode_for(root, name));
                    self.apply_video_geometry();
                    self.screen_power = 0.0;
                    self.exiting_screen = false;
                    self.gpu.set_screen_power(0.0);
                    PLAYING.store(true, Ordering::Release);
                }
            }
        }

        if self.game.is_some() && matches!(self.overlay, ShelfOverlay::States) {
            // Polaroids are a full-screen, paused overlay. Render once per
            // display frame (not from a key event) and never advance libretro
            // or enqueue audio while browsing saved states.
            self.game_accum = 0.0;
            let mut commands = Vec::new();
            if let Some(p) = self.polaroids.as_mut() {
                p.set_undo(self.game.as_ref()
                    .is_some_and(|game| game.undo_available())
                    .then_some("undo"));
                p.draw(None, Printed::default(), None, Printed::default(), &mut commands);
            }
            self.gpu.fit(self.size);
            self.gpu.begin_frame();
            self.gpu.draw_list(&commands);
            self.gpu.end_frame(self.size);
            return;
        }
        if self.game.is_some() && matches!(self.overlay,
            ShelfOverlay::GameMenu { .. } | ShelfOverlay::Multiplayer |
            ShelfOverlay::PauseSettings { .. } | ShelfOverlay::PauseScreenSettings { .. } |
            ShelfOverlay::PauseGameplaySettings { .. } | ShelfOverlay::PauseEmulationSettings { .. }) {
            // Coordinate the pause with the remote game. Without this
            // the other Cable peer times out and gpSP RFU can drift.
            if let Some(game) = self.game.as_mut() { game.set_link_paused(true); }
            // No libretro frames are evaluated while a native overlay is
            // visible. Poll transport atomics here as well; otherwise a remote
            // End Link or a broken socket is invisible until the local player
            // closes the menu, potentially leaving a permanent "Linked" UI.
            if let Some(fault) = self.game.as_ref().and_then(GameSession::link_fault) {
                // Peer End/EOF returns to SOLO in-place; don't display
                // a misleading transient link-ended banner over gameplay.
                if !matches!(fault,
                    crate::link_play::LinkError::PeerEnded |
                    crate::link_play::LinkError::Disconnected) {
                    self.hud.toast(link_fault_toast(fault),
                        self.born.elapsed().as_millis() as u64);
                }
                self.ff_held = false;
                self.ff_latched = false;
                self.rewind_held = false;
                self.hud.set_ff(FfState::Off);
                AUDIO_SPEED_PERMILLE.store(1000, Ordering::Release);
                let rejoined = if matches!(fault,
                    crate::link_play::LinkError::PeerEnded |
                    crate::link_play::LinkError::Disconnected) {
                    self.return_link_to_solo()
                } else {
                    Err(format!("Unsafe Link transport: {fault:?}"))
                };
                match rejoined {
                    Ok(()) => {
                        // Peer unplugged; this cartridge keeps its local
                        // SRAM and full solo state without an app eject.
                        self.hud.set_link(LinkBadge::Off);
                        self.overlay = ShelfOverlay::None;
                    }
                    Err(reason) => {
                        if let Some(game) = self.game.as_mut() { game.end_multiplayer(); }
                        self.hud.set_link(link_badge_for(self.link_role, true));
                        if self.game.is_some() {
                            self.overlay = ShelfOverlay::GameMenu { row: 0 };
                        } else { self.handle(Input::Exit); }
                        set_message(format!("Multiplayer stopped: {reason}"));
                    }
                }
            }
            self.game_accum = 0.0;
            let mut commands = vec![Draw::Game];
            self.draw_overlay(&mut commands);
            // The original Slot app draws the HUD AFTER draw_game_menu()
            // (app.rs), including during its Linked animation. The four
            // cyan/purple chain badges are already loaded into our Hud but
            // used to disappear behind the Multiplayer overlay because we
            // skipped this final HUD pass. Keep the exact upstream z-order.
            if matches!(self.overlay, ShelfOverlay::Multiplayer) {
                self.hud.draw(self.born.elapsed().as_millis() as u64, &mut commands);
            }
            self.gpu.fit(self.size);
            self.gpu.begin_frame();
            self.gpu.draw_list(&commands);
            self.gpu.end_frame(self.size);
            return;
        }
        if self.game.is_some() && self.select_down_at.is_some_and(
            |at| at.elapsed().as_millis() >= SELECT_CHORD_MS)
            && !self.select_chorded && !self.select_forwarded
        {
            self.select_forwarded = true;
            self.buttons |= ButtonMask::SELECT;
        }
        let profile = self.current_emulation();
        let bezel=self.game_bezel(); // resolved before mutably borrowing GameSession
        let live=self.live_game_options(); // Copy, zero allocations on 60/120Hz callback
        FF_AUDIO_ENABLED.store(live.ff_sound,Ordering::Release);
        // Android's frame callback changes GLSurfaceView renderMode only when
        // an active game explicitly selected DISPLAY SYNC. No engine work or
        // GL resource ownership ever moves away from the GL render thread.
        DISPLAY_SYNC.store(self.game.is_some() && profile.display_sync_pacing,
            Ordering::Release);
        // Grab clear-cart textures before borrowing the mutable emulator
        // session. They are used only for the 220ms CRT start transition.
        let (transition_back, transition_front) = self.clear_layers();
        let a12_native_started = Instant::now();
        if let Some(session) = self.game.as_mut() {
            // Resume only after the remote has also left its pause menu;
            // LinkPlay::frame holds either core while peer_paused is true.
            session.set_link_paused(false);
            // In-game HUD replaces the shelf chrome; never let the upper
            // Android indicators remain visible on the first game frame.
            SHELF_INDICATOR_FLAGS.store(0, Ordering::Release);
            let mut stepped = 0;
            let mut peer_unplugged = None;
            if self.rewind_held {
                // Rewind is mutually exclusive with forward libretro frames.
                // Restore up to 3 historical snapshots per render frame and
                // never pass rewound audio into the normal AudioTrack.
                self.game_accum = 0.0;
                self.rewind_accum = (self.rewind_accum + dt).min(0.30);
                while self.rewind_accum >= REWIND_STEP_S && stepped < 3 {
                    self.rewind_accum -= REWIND_STEP_S;
                    if !session.rewind_step() {
                        self.rewind_accum = 0.0;
                        break;
                    }
                    stepped += 1;
                }
                AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
                self.hud.show(HudKind::Rewind, session.rewind_fill(), false,
                    self.born.elapsed().as_millis() as u64);
            } else {
                // Original Slot offers 2/3/4/6x, independent of display Hz.
                let fast = self.ff_held || self.ff_latched;
                let multiplier = if fast { f64::from(live.ff_speed) } else { 1.0 };
                let period = 1.0 / session.fps;
                // RC4 Beta 5 restores Beta 3's less aggressive RESPONSIVE mode:
                // never insert catch-up frames on a single display callback.
                // A slightly lower EMU FPS is preferable to observable bursts
                // and inconsistent input response on the physical KONKR.
                // STANDARD and fast-forward are not changed.
                let responsive = profile.responsive_pacing && !fast;
                // A11: this is the *same* double-GBA lockstep on both ends,
                // even for equal ROMs. One late packet must not make gameplay
                // and audio permanently lose an emulated frame. Restrict
                // recovery to mGBA Cable; gpSP and solo retain their policy.
                let mgba_cable_pacing = session.linked()
                    && session.core_kind() == Core::Mgba && !fast
                    && MGBA_CABLE_RECOVERY.load(Ordering::Acquire);
                // Preserve a little more fractional game time in the
                // opt-in DISPLAY SYNC mode ONLY. No extra libretro frames
                // can run in any single callback; Beta 4 catch-up stays gone.
                if mgba_cable_pacing {
                    self.game_accum = crate::settings::frame_debt_mgba_cable(
                        self.game_accum, f64::from(dt), period);
                } else if profile.display_sync_pacing && responsive {
                    self.game_accum = crate::settings::frame_debt_display_sync(
                        self.game_accum, f64::from(dt), multiplier, period);
                } else {
                    self.game_accum = crate::settings::frame_debt(
                        self.game_accum, f64::from(dt), multiplier, period, fast, responsive);
                }
                let frame_limit = if mgba_cable_pacing { 2 }
                    else if fast { usize::from(live.ff_speed) * 4 }
                    else if responsive { 1 } else { 4 };
                // A13.1: revert the experimental second-frame budget.
                // Real-world A13 KONKR/AVD tests suffered repeated Link
                // wait_peer stalls and severe AudioTrack starvation.
                // A12's A11 bounded-debt/2-frame policy was smoother.
                while self.game_accum >= period && stepped < frame_limit {
                    // Cable lockstep must not consume emulation time while
                    // the remote frame has not arrived. NEVER invent a solo
                    // frame or perform speculative Run-Ahead on a held link.
                    if !session.advance(self.buttons, live.turbo,
                        live.rewind && !session.linked()) {
                        // A waiting mGBA Cable frame is NOT an emulated frame.
                        // Retain at most two frames of debt; only real remote
                        // inputs can advance them later. Never preserve debt
                        // after an actual Link fault or in solo/gpSP.
                        self.game_accum = if mgba_cable_pacing
                            && session.link_fault().is_none() {
                            self.game_accum.min(period * 2.0)
                        } else { 0.0 };
                        if let Some(fault) = session.link_fault() {
                            // Remote End Link and TCP loss can become a
                            // verified single-GBA SOLO instance after the
                            // live session's final frame has been drawn.
                            // Invalid/corrupt frames remain fail-closed.
                            if matches!(fault, crate::link_play::LinkError::PeerEnded |
                                crate::link_play::LinkError::Disconnected) {
                                peer_unplugged = Some(fault);
                            } else {
                                session.end_multiplayer();
                                self.hud.set_link(link_badge_for(self.link_role, true));
                                self.hud.toast(link_fault_toast(fault),
                                    self.born.elapsed().as_millis() as u64);
                                self.overlay = ShelfOverlay::GameMenu { row: 0 };
                                set_message(format!("Multiplayer paused: {fault:?}"));
                            }
                            self.ff_held = false;
                            self.ff_latched = false;
                            self.rewind_held = false;
                            self.hud.set_ff(FfState::Off);
                            AUDIO_SPEED_PERMILLE.store(1000, Ordering::Release);
                        }
                        break;
                    }
                    self.perf_emulated = self.perf_emulated.saturating_add(1);
                    self.game_accum -= period;
                    stepped += 1;
                    let samples = session.take_audio();
                    if (!fast || (live.ff_sound &&
                        FF_AUDIO_SUPPORTED.load(Ordering::Acquire)))
                        && !samples.is_empty() {
                        let mut audio = AUDIO.lock().unwrap_or_else(|e| e.into_inner());
                        let overflow = audio.len().saturating_add(samples.len()).saturating_sub(96_000);
                        for _ in 0..overflow.min(audio.len()) { audio.pop_front(); }
                        audio.extend(samples);
                    }
                }
            }
            if stepped > 0 && !self.rewind_held && !self.ff_held && !self.ff_latched
                && profile.run_ahead_frames > 0 {
                // The normal frames and their audio are already consumed.
                // Predicted video never advances the real saved/RA timeline.
                session.prepare_runahead(self.buttons, live.turbo,
                    profile.run_ahead_frames);
            }
            if stepped > 1 {
                self.perf_catchup = self.perf_catchup.saturating_add(1);
            }
            if stepped > 0 {
                if self.select_tap_frames > 0 {
                    self.select_tap_frames = self.select_tap_frames.saturating_sub(stepped as u8);
                    if self.select_tap_frames == 0 { self.buttons &= !ButtonMask::SELECT; }
                }
                self.gpu.upload_game(session.frame());
                RUMBLE_STRENGTH.store(
                    if live.rumble && !self.rewind_held {
                        i32::from(session.rumble_strength())
                    } else { 0 },
                    Ordering::Release,
                );
            }
            self.gpu.fit(self.size);
            self.gpu.begin_frame();
            let mut commands = Vec::new();
            if self.screen_power < 1.0 {
                // Native Slot v1.7 draws the seated cart and slot behind the
                // CRT picture while the LCD grows to full width (220ms).
                // We use the existing GamePass screen-power shader, without
                // adding a separate full-screen fade or extra libretro frames.
                let shelf = &self.shelves[self.active];
                if let Some(cart) = shelf.carts.get(shelf.index) {
                    let (rest, scale) = shelf.selected_at();
                    slot_ui::draw_ground_tinted(None, 0.0, &mut commands);
                    SlotChrome {
                        cart, face: shelf.face(shelf.index), rest, scale,
                        seat: self.progress, alert: None, dim: 0.0,
                        screen: self.screen_power, game: true, over: false,
                        slot_faces: self.slot_faces,
                    }.draw_with_material(&mut commands, transition_back, transition_front);
                } else {
                    commands.push(Draw::Game);
                }
            } else {
                commands.push(Draw::Game);
            }
            if let Some(tex)=bezel {
                commands.push(Draw::Tex {
                    x:0.0,y:0.0,w:OUT_W as f32,h:OUT_H as f32,
                    tex,alpha:1.0
                });
            }
            self.hud.draw(self.born.elapsed().as_millis() as u64, &mut commands);
            self.gpu.draw_list(&commands);
            self.gpu.end_frame(self.size);
            a12_draw_sample(session.linked() && session.core_kind() == Core::Mgba,
                stepped, a12_native_started.elapsed().as_micros() as u64,
                MGBA_CABLE_RECOVERY.load(Ordering::Acquire));
            if let Some(_fault) = peer_unplugged {
                // Silent, verified solo return on intentional unplug or TCP EOF.
                match self.return_link_to_solo() {
                    Ok(()) => {
                        self.hud.set_link(LinkBadge::Off);
                        self.overlay = ShelfOverlay::None;
                    }
                    Err(reason) => {
                        if let Some(game) = self.game.as_mut() { game.end_multiplayer(); }
                        self.hud.set_link(link_badge_for(self.link_role, true));
                        if self.game.is_some() {
                            self.overlay = ShelfOverlay::GameMenu { row: 0 };
                        } else { self.handle(Input::Exit); }
                        set_message(format!("Unable to recover Link player: {reason}"));
                    }
                }
            }
            return;
        }

        let ms = self.born.elapsed().as_millis() as u64;
        let repeated_letter = if !self.inserted && matches!(self.overlay,ShelfOverlay::None) {
            self.shelves[self.active].tick(ms)
        } else {
            self.shelves[self.active].release_hold();
            false
        };
        if repeated_letter { self.show_selected_letter(); }
        self.shelves[self.active].update(dt);
        self.prepare_visible();
        // Only warm a settled carousel selection. Actual ROM I/O takes place
        // on an independently cancelable Android background executor.
        let warming = !self.inserted && self.game.is_none()
            && self.pending_game.is_none() && !self.awaiting_game
            && self.progress <= 0.0
            && matches!(self.overlay, ShelfOverlay::None)
            && !self.shelves[self.active].is_scrolling()
            && self.last_cart_navigation.elapsed() >= Duration::from_millis(900);
        let candidate = if warming {
            self.shelves[self.active].carts
                .get(self.shelves[self.active].index)
                .map(|cart|cart.rom.to_string_lossy().into_owned())
        } else { None };
        let mut last = WARM_ROM_URI.lock().unwrap_or_else(|e|e.into_inner());
        if *last != candidate { *last = candidate; }
        drop(last);
        self.refresh_selected_title();
        self.persist_focus_if_idle();
        // Resolve the optional transparent material before mutably borrowing
        // the selected rack, otherwise Rust rejects the overlapping borrows.
        let (selected_clear_back, selected_clear_front) = self.clear_layers();
        let shelf = &mut self.shelves[self.active];

        // Original Slot: 450ms seating + 280ms hold. Start the original
        // insert clip 97ms before full contact; its embedded lead completes
        // exactly at the seating point. Ejection follows CRT-off + black hold.
        let previous_progress = self.progress;
        self.progress = advance_cart(self.progress, self.inserted, dt);
        if self.inserted && previous_progress < INSERT_SOUND_PROGRESS
            && self.progress >= INSERT_SOUND_PROGRESS
        {
            CART_SFX.lock().unwrap_or_else(|e| e.into_inner()).push_back(CART_INSERT_SFX);
        }
        if !self.inserted && self.eject_sound_armed
            && self.progress < previous_progress
            && self.progress <= EJECT_SOUND_PROGRESS {
            self.eject_sound_armed = false;
            CART_SFX.lock().unwrap_or_else(|e| e.into_inner()).push_back(CART_EJECT_SFX);
        }

        let mut commands = vec![Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: [0.085, 0.085, 0.093, 1.0],
        }];
        // Slot v1.7 carousel ground: None has no wallpaper; personal images
        // receive only the selected Wallpaper Tint (never a fixed scrim).
        let active_wallpaper = if self.settings.wallpaper ==
            crate::settings::WallpaperPick::None { None }
            else { self.wallpaper_texture };
        let tint = if self.wallpaper_personal {
            self.settings.wallpaper_tint.alpha()
        } else { 0.0 };
        slot_ui::draw_ground_tinted(active_wallpaper, tint, &mut commands);
        if self.progress == 0.0 {
            if let Some(picker) = self.picker {
                let t = slot_ui::ease(picker.openness(self.born.elapsed().as_millis() as u64));
                let dim = 1.0 + (0.614 - 1.0) * t;
                let selected = shelf.carts.get(shelf.index).map(|c| c.stem.as_str());
                shelf.draw_row(selected, 0.0, 0.26 * t, dim, &mut commands);
                slot_ui::draw_empty_slot_with(self.slot_faces, &mut commands);
            } else {
                // Match upstream Refusal: only the shelf carts shake.
                // The physical slot stays fixed; no input delay or layout changes.
                let shake = self.refusal.map_or(0.0, |r| r.offset(ms));
                shelf.draw_row(None, shake, 0.0, 1.0, &mut commands);
                slot_ui::draw_empty_slot_with(self.slot_faces, &mut commands);
                // Official v1.7.0 shelf: system (L1/R1) or letter
                // (D-pad Up/Down) uses a 200/1200/800 ms full-opacity name.
                // Game Titles crossfade back in during the last 800ms, and
                // stay visible after the name expires. Plain Left/Right
                // scrolling swaps the title immediately with no name fade.
                let name = if let Some(since) = self.platform_shown_at {
                    let elapsed=since.elapsed().as_millis() as u64;
                    if platform_name_running(elapsed) {
                        let face=match self.active {
                                3=>self.recents_platform_face,
                                4=>self.favorites_platform_face,
                                index=>self.platform_faces[index],
                            };
                        Some((face,elapsed,true))
                    } else {
                        self.platform_shown_at=None;
                        None
                    }
                } else if let Some(since)=self.letter_shown_at {
                    let elapsed=since.elapsed().as_millis() as u64;
                    if platform_name_running(elapsed) {
                        Some((self.letter_face,elapsed,false))
                    } else {
                        self.letter_shown_at=None;
                        None
                    }
                } else {None};
                if let Some((face,elapsed,is_platform))=name {
                    let alpha=slot_name_alpha(elapsed);
                    if alpha>0.0 {
                        if is_platform {draw_platform_name(face,alpha,&mut commands);}
                        else {draw_slot_name(face,alpha,&mut commands);}
                    }
                }
                if self.settings.show_titles {
                    let alpha=name.map_or(1.0,|(_,elapsed,_)|slot_title_alpha(elapsed));
                    slot_ui::draw_slot_title(self.selected_title_face,alpha,&mut commands);
                }
            }
        } else {
            let cart = &shelf.carts[shelf.index];
            let (rest, scale) = shelf.selected_at();
            // Identical 450ms recede/return as upstream slot. v1.7.
            // Use the existing seat progress; never alter the ROM or game timing.
            shelf.draw_row(Some(cart.stem.as_str()), 0.0, self.progress, 1.0, &mut commands);
            SlotChrome {
                cart,
                face: shelf.face(shelf.index),
                rest,
                scale,
                seat: self.progress,
                alert: None,
                dim: 0.0,
                screen: 0.0,
                game: false,
                over: true,
                slot_faces: self.slot_faces,
            }
            .draw_with_material(&mut commands,selected_clear_back,selected_clear_front);
        }
        // The onboarding keycap belongs to Slot's own rendering lifecycle,
        // never the Android overlay. No hint is drawn behind an open menu.
        if self.progress == 0.0 && matches!(self.overlay, ShelfOverlay::None) {
            if self.shelf_status > 0 {
                let (tex, w, h) = self.shelf_status_faces[usize::from(self.shelf_status)];
                commands.push(Draw::Tex {
                    x: (OUT_W as f32 - w as f32) / 2.0,
                    // Keep status text below the Android corner indicators.
                    y: 74.0,
                    w: w as f32, h: h as f32,
                    tex, alpha: 1.0,
                });
                if self.shelf_status != 1 {
                    // Exactly the upstream Slot START keycap; menus hide both
                    // messages and keycaps rather than leaving Android overlays.
                    let (tex,w)=if self.shelf_status==2 {
                        self.shelf_no_games_hint
                    } else {
                        self.shelf_status_start
                    };
                    commands.push(Draw::Tex {
                        x: (OUT_W as f32 - w as f32) / 2.0,
                        y: 110.0,
                        w: w as f32, h: HINT_H as f32,
                        tex, alpha: 1.0,
                    });
                    if self.shelf_status==2 {
                        let (tex,w)=self.onboarding_note;
                        commands.push(Draw::Tex {
                            x: (OUT_W as f32 - w as f32) / 2.0,
                            y: 140.0,
                            w: w as f32, h: HINT_H as f32,
                            tex, alpha: 0.76,
                        });
                    }
                }
            } else if self.shelves.iter().all(|s| s.carts.is_empty()) {
                let (tex, w) = self.first_run_hint;
                commands.push(Draw::Tex {
                    x: (OUT_W as f32 - w as f32) / 2.0,
                    y: 34.0,
                    w: w as f32,
                    h: HINT_H as f32,
                    tex, alpha: 1.0,
                });
                let (tex,w)=self.onboarding_note;
                commands.push(Draw::Tex {
                    x: (OUT_W as f32 - w as f32) / 2.0,
                    y: 64.0,
                    w: w as f32, h: HINT_H as f32,
                    tex, alpha: 0.76,
                });
            }
        }
        self.draw_overlay(&mut commands);
        // Keep the upper indicators on the identical visibility lifecycle
        // as Slot's footer: only on an idle, menu-free cartridge carousel.
        let chrome = self.progress == 0.0 && matches!(self.overlay, ShelfOverlay::None);
        let mut indicator_flags = 0;
        if chrome {
            indicator_flags |= 1; // show upper indicators
            if self.settings.show_wifi { indicator_flags |= 2; }
            if self.settings.show_bluetooth { indicator_flags |= 4; }
            if self.settings.show_integrations { indicator_flags |= 8; }
            let light=self.settings.appearance==crate::settings::Appearance::Light;
            slot_ui::draw_footer_with_ink(self.battery,self.battery_face,
                Some(if light {self.bolt_light_texture} else {self.bolt_texture}),
                self.clock_face,self.shelf_footer_ink(),&mut commands);
        }
        SHELF_INDICATOR_FLAGS.store(indicator_flags, Ordering::Release);
        self.gpu.fit(self.size);
        self.gpu.begin_frame();
        self.gpu.draw_list(&commands);
        self.gpu.end_frame(self.size);
        // Idle throttle is an output-only pacing hint; never change upstream
        // animation timings, input handling, or cartridge prefetch priorities.
        let quiet = !self.inserted
            && self.game.is_none() && self.pending_game.is_none()
            && !self.awaiting_game && !self.external_pending
            && self.progress <= 0.0
            && matches!(self.overlay, ShelfOverlay::None)
            && self.shelf_status != 1 // library scanner progress remains animated
            && self.last_cart_navigation.elapsed() >= Duration::from_millis(3200)
            && self.menu_opened.elapsed() >= Duration::from_millis(1200)
            && !self.shelves[self.active].is_scrolling()
            && self.cart_render.is_idle();
        CAROUSEL_QUIET.store(quiet, Ordering::Release);
    }
}

/// Do not report an unsuccessful core reopen as "solo restored".
/// Always surface the genuine checkpoint error to the user: the durable
/// solo state and battery files remain on disk for safe cartridge relaunch.
/// Match the upstream Eject-first rule even if Android onPause occurs
/// outside the Multiplayer menu. Pure so its fullscreen branch cannot
/// silently regress when editing the lifecycle handling.
fn suspend_returns_to_eject_menu(requires_relaunch: bool,
                                 menu_is_multiplayer: bool) -> bool {
    requires_relaunch || menu_is_multiplayer
}

fn link_boot_failure(context: &str, error: &str,
                     recovery: Result<(), String>) -> String {
    match recovery {
        Ok(()) => format!("{context}: {error}; original solo progress restored"),
        Err(cause) => format!("{context}: {error}; solo recovery failed: {cause}. Eject/relaunch the cartridge; saved progress remains on disk"),
    }
}

// The source-of-truth coordinates and width accounting from upstream
// BrandonKowalski/slot app.rs: draw_game_menu(). Do not replace the
// original 720x480 centred keycaps with the Android Settings footer or
// a second full-width black rectangle at y=420.
const LINK_TEXT_Y: f32 = 44.0;
const LINK_LEGEND_Y: f32 = 422.0;
const LINK_LEGEND_GAP: f32 = 40.0;

fn link_heading_x(width: u32) -> f32 {
    ((OUT_W as f32 - width as f32) / 2.0).round()
}

fn link_legend_start(hints: &[(slot_gfx::TexId, u32)]) -> f32 {
    let total = hints.iter().map(|(_, w)| w.saturating_sub(HINT_EDGE) as f32)
        .sum::<f32>() + LINK_LEGEND_GAP * hints.len().saturating_sub(1) as f32;
    ((OUT_W as f32 - total) / 2.0).round()
}

/// Original Slot / fork draw_game_menu() picks exactly these groups of
/// bottom keycaps. Keep selection separate from the GL scene so changes
/// cannot accidentally show A Link while searching/linked or conceal
/// SELECT Mode on a real gpSP RFU/Cable switchable title.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum LinkLegendProfile {
    PickerWithMode, // B CANCEL | SELECT MODE | arrows SWAP | A LINK
    Picker,         // B CANCEL | arrows SWAP | A LINK
    Searching,      // B CANCEL
    ConnectedMenu,  // B BACK | A END LINK (only reopened overlay)
    Failed,         // A OK
    None,           // initial LINKED / animated Unplug
}

fn link_legend_profile(menu: LinkMenu, selectable_mode: bool) -> LinkLegendProfile {
    match menu {
        LinkMenu::Pick(_) if selectable_mode => LinkLegendProfile::PickerWithMode,
        LinkMenu::Pick(_) => LinkLegendProfile::Picker,
        LinkMenu::Working { .. } => LinkLegendProfile::Searching,
        LinkMenu::Linked { opened: true, .. } => LinkLegendProfile::ConnectedMenu,
        LinkMenu::Linked { opened: false, .. } | LinkMenu::Unplug { .. } =>
            LinkLegendProfile::None,
        LinkMenu::Failed { .. } => LinkLegendProfile::Failed,
    }
}

/// A Link Mode toggle must never lie about actual physical/serial support.
fn link_mode_can_switch(core: Core, chosen: LinkKind, code: &str,
                        title: &str, clean: bool) -> bool {
    if core != Core::Gpsp || !link_kind::link_carried(code, title)
        || !link_kind::has_wireless_adapter(code)
        || !link_kind::gpsp_runtime_mode_supported(code, chosen)
        || !link_kind::gpsp_runtime_mode_supported(code, chosen.other()) { return false; }
    let automatic = link_kind::link_kind(code, title, clean);
    let selected_serial = link_kind::serial_option(chosen, automatic, code, title);
    let opposite_serial = link_kind::serial_option(chosen.other(), automatic, code, title);
    selected_serial != opposite_serial
}

/// These rows are modelled by *action*, not by a hardcoded screen index.
/// It prevents a linked session from routing "Save & Eject" to "Resume".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PauseRow { Resume, States, Multiplayer, Settings, SaveEject }
const SOLO_PAUSE_ROWS: &[PauseRow] = &[
    PauseRow::Resume, PauseRow::States, PauseRow::Multiplayer,
    PauseRow::Settings, PauseRow::SaveEject,
];
const LINK_PAUSE_ROWS: &[PauseRow] = &[
    PauseRow::Resume, PauseRow::Multiplayer, PauseRow::Settings,
    PauseRow::SaveEject,
];
// If a Link core must be relaunched before solo, it must not expose
// unsafe actions or a misleading Resume control.
const RELAUNCH_PAUSE_ROWS: &[PauseRow] = &[
    PauseRow::SaveEject, PauseRow::Multiplayer,
];
fn pause_rows(linked: bool, requires_relaunch: bool) -> &'static [PauseRow] {
    if requires_relaunch { RELAUNCH_PAUSE_ROWS }
    else if linked { LINK_PAUSE_ROWS }
    else { SOLO_PAUSE_ROWS }
}
fn pause_row_text(row: PauseRow, linked: bool, requires_relaunch: bool) -> usize {
    match row {
        PauseRow::Resume => 20,
        PauseRow::States => 22,
        PauseRow::Multiplayer => 66, // Entry always opens status; End Link stays inside adapter
        PauseRow::Settings => 25,
        PauseRow::SaveEject if requires_relaunch => 78,
        PauseRow::SaveEject => 21,
    }
}

/// Ejection and resume are never inferred from their displayed row numbers.
fn game_menu_ejects(row: usize, linked: bool, requires_relaunch: bool) -> bool {
    matches!(pause_rows(linked, requires_relaunch).get(row), Some(PauseRow::SaveEject))
}

/// The original Slot HUD distinguishes a remote end and a BIOS mismatch.
fn link_fault_toast(fault: crate::link_play::LinkError) -> Toast {
    match fault {
        crate::link_play::LinkError::BiosMismatch => Toast::BiosMismatch,
        crate::link_play::LinkError::PeerEnded
        | crate::link_play::LinkError::Disconnected
        | crate::link_play::LinkError::InvalidState
        | crate::link_play::LinkError::Timeout => Toast::PeerEnded,
    }
}

/// Map the negotiated on-wire role to upstream Slot Link icon variants.
fn link_badge_for(role: LinkRow, disconnected: bool) -> LinkBadge {
    match (role, disconnected) {
        (LinkRow::Host, false) => LinkBadge::Hosting,
        (LinkRow::Join, false) => LinkBadge::Joined,
        (LinkRow::Host, true) => LinkBadge::HostingLost,
        (LinkRow::Join, true) => LinkBadge::JoinedLost,
    }
}

/// Only the initial "Linked" seating animation is self-dismissing.
/// Reopening from SELECT+HOME must keep End Link/Back visible until input.
fn link_menu_auto_closes(phase: LinkMenu, now: u64) -> bool {
    matches!(phase, LinkMenu::Linked { since, opened: false, .. }
        if now.saturating_sub(since) >= 1000)
}

/// Use the exact 260ms Slot link adapter unplug animation.
fn link_unplug_complete(phase: LinkMenu, now: u64) -> bool {
    // Upstream: the hardware withdraws in 260ms, then holds the screen
    // another 160ms before exiting the Link menu (420ms total).
    matches!(phase, LinkMenu::Unplug { since, .. }
        if now.saturating_sub(since) >= 420)
}

/// A library refresh may never terminate an inserted or pending game.
fn may_apply_library_update(game_active: bool, inserted: bool, awaiting: bool) -> bool {
    !game_active && !inserted && !awaiting
}

/// A clean install has three shelves but zero cartridges. Never enqueue
/// nonexistent artwork for the first draw (or after clearing the library).
fn selected_prefetch_entry(shelves: &[Shelf], active: usize) -> Option<(usize, usize)> {
    let shelf = shelves.get(active)?;
    shelf.carts.get(shelf.index).map(|_| (active, shelf.index))
}

/// Restart on the exact platform + cart when possible; if a hidden/removed
/// Recents or Favorites rack no longer contains it, recover the source cart.
fn restore_saved_focus(shelves:&mut [Shelf], focus:&crate::selection::Focus)->Option<usize> {
    let candidate=shelves.get(focus.rack).and_then(|shelf|
        shelf.carts.iter().position(|cart|cart.rom.to_string_lossy()==focus.uri.as_str()));
    if let Some(index)=candidate {
        shelves[focus.rack].select(index);
        return Some(focus.rack);
    }
    for rack in 0..3 {
        if let Some(index)=shelves.get(rack).and_then(|shelf|
            shelf.carts.iter().position(|cart|cart.rom.to_string_lossy()==focus.uri.as_str())) {
            shelves[rack].select(index);
            return Some(rack);
        }
    }
    None
}

/// Preserve stable ROM identity and platform on a Cart Studio shelf rebuild.
/// Missing/removed games safely fall back to the first available cartridge.
fn restore_shelf_focus(shelves: &mut [Shelf], active: usize, uris: &[Option<PathBuf>]) -> usize {
    for (shelf, wanted) in shelves.iter_mut().zip(uris.iter()) {
        if let Some(path) = wanted {
            if let Some(index) = shelf.carts.iter().position(|cart| cart.rom == *path) {
                shelf.select(index);
            }
        }
    }
    if shelves.get(active).is_some_and(|shelf| !shelf.carts.is_empty()) {
        active
    } else {
        next_nonempty_shelf(shelves, active.min(shelves.len().saturating_sub(1)), 1)
    }
}

/// Slot. keeps the label timer alive even when fade-in alpha is initially zero.
/// Time-based expiry (not alpha-based) is essential for reliable GBA startup
/// and every L1/R1 GB/GBC/GBA switch on a high-refresh-rate Android panel.
fn platform_name_running(elapsed_ms: u64) -> bool {
    elapsed_ms < 200 + 1200 + 800
}

/// Official upstream v1.7 name / title animation is full opacity (1.0).
/// Hide titles until 1400ms, then crossfade during the 800ms fade-out.
fn slot_title_alpha(elapsed_ms: u64) -> f32 {
    if elapsed_ms < 1400 { return 0.0; }
    (1.0 - slot_name_alpha(elapsed_ms)).clamp(0.0, 1.0)
}

fn slot_name_alpha(elapsed_ms: u64) -> f32 {
    let level = if elapsed_ms < 200 {
        elapsed_ms as f32 / 200.0
    } else if elapsed_ms < 1400 {
        1.0
    } else {
        (1.0 - (elapsed_ms - 1400) as f32 / 800.0).max(0.0)
    };
    slot_ui::ease(level)
}

/// Physical D-pad UP and DOWN must move the selection in opposite
/// directions, including wraparound; never leak navigation to libretro.
fn move_menu_row(row: usize, key: i32, count: usize) -> usize {
    if count == 0 { return 0; }
    match key {
        19 => (row + count - 1) % count,
        20 => (row + 1) % count,
        _ => row % count,
    }
}

/// Match Slot's centred menu row layout on the 720x480 design canvas.
fn settings_row_top(rows: usize) -> f32 {
    ((OUT_H as f32 - QUICK_PITCH * rows as f32) / 2.0).round()
}

/// Same easing timeline for the shelf and CRT-composited exit frames.
/// Separating progress from rendering makes the animation durations testable.
fn advance_cart(progress: f32, inserted: bool, dt: f32) -> f32 {
    if inserted {
        (progress + dt / SEATED_AT).min(1.0)
    } else {
        (progress - dt / EJECT_S).max(0.0)
    }
}

/// Return the current shelf if it is the only populated one, and never
/// select an empty GB/GBC/GBA shelf. Safe even if all libraries are empty.
fn next_nonempty_shelf(shelves: &[Shelf], current: usize, delta: i32) -> usize {
    let counts: Vec<usize> = shelves.iter().map(|s| s.carts.len()).collect();
    crate::library::next_populated(current, delta, &counts)
}

#[cfg(test)]
mod ui_feedback_tests {
    use super::*;

    #[test]
    fn gba_three_two_screen_hides_no_op_bezel_but_keeps_gb_gbc() {
        assert_eq!(Engine::screen_rows(Platform::Gba), &[34,29]);
        assert_eq!(Engine::screen_keys(Platform::Gba), &[0,22]);
        assert_eq!(Engine::screen_rows(Platform::Gbc), &[88,29,90]);
        assert_eq!(Engine::screen_rows(Platform::Gb), &[89,29,36,90]);
    }
    #[test]
    fn ordinary_cart_scrolling_does_not_restart_name_fade() {
        // A no-name-transition title is drawn at alpha 1 immediately;
        // only a letter jump invokes slot_title_alpha().
        assert_eq!(slot_title_alpha(0), 0.0);
        assert_eq!(slot_title_alpha(2200), 1.0);
        assert!(!platform_name_running(2200));
    }
    #[test]
    fn official_game_title_fades_after_name_and_then_remains_visible() {
        // The earlier U1 renderer incorrectly expired Game Titles after
        // 2200ms; upstream instead keeps a fully visible title at rest.
        assert_eq!(slot_title_alpha(0), 0.0);
        assert_eq!(slot_title_alpha(1399), 0.0);
        assert!((slot_title_alpha(1800) - 0.5).abs() < 0.0001);
        assert_eq!(slot_name_alpha(200), 1.0);
        assert_eq!(slot_name_alpha(1400), 1.0);
        assert!((slot_name_alpha(1800) - 0.5).abs() < 0.0001);
        assert_eq!(slot_title_alpha(2200), 1.0);
        assert_eq!(slot_title_alpha(100_000), 1.0);
    }


    #[test]
    fn legacy_multiplayer_heading_helper_mode_and_footer_text_cannot_come_back() {
        // Our previous integration/konkr-rc4-next rendered indices 68..77
        // as extra MULTIPLAYER/CABLE/CHOOSE... overlays, together with an
        // arbitrary 60px black footer and LEFT/RIGHT text keycap.
        // They remain reserved index slots so unrelated menus are stable,
        // but no legacy Multiplayer string can be rendered from MENU_TEXT.
        assert!(MENU_TEXT[68..=77].iter().all(|label| label.is_empty()));
        assert_eq!(MENU_TEXT[66], "Multiplayer"); // still a game menu action
        assert_eq!(MENU_TEXT[67], "End Link");
        assert_eq!(MENU_TEXT[78], "Eject to play solo");
    }

    #[test]
    fn original_slot_link_title_and_bottom_keycaps_use_exact_canvas_geometry() {
        assert_eq!(OUT_W, 720);
        assert_eq!(OUT_H, 480);
        assert_eq!(LINK_TEXT_Y, 44.0);
        assert_eq!(LINK_LEGEND_Y, 422.0);
        assert_eq!(LINK_LEGEND_GAP, 40.0);
        for width in [88, 100, 120, 216, 300] {
            let x = link_heading_x(width);
            assert!(((x + width as f32 * 0.5) - 360.0).abs() <= 0.5);
        }
        // The old HOST was hard-coded to x=300 regardless of typography;
        // this rule always centres both translated role title glyph widths.
        assert_eq!(link_heading_x(120), 300.0);
        assert_eq!(link_heading_x(216), 252.0);
    }

    #[test]
    fn android_suspend_forces_eject_menu_when_a_live_link_ended_fullscreen() {
        assert!(!suspend_returns_to_eject_menu(false, false),
            "an ordinary solo game can resume without a modal");
        assert!(suspend_returns_to_eject_menu(false, true),
            "discovery interrupted onPause must leave Multiplayer");
        assert!(suspend_returns_to_eject_menu(true, false),
            "linked gameplay interrupted fullscreen must require Eject");
        assert!(suspend_returns_to_eject_menu(true, true));
    }

    #[test]
    fn link_boot_failures_never_pretend_solo_recovery_succeeded() {
        let ok = link_boot_failure("gpSP Link", "cannot connect", Ok(()));
        assert!(ok.contains("original solo progress restored"));
        let unsafe_state = link_boot_failure("mGBA Link", "invalid peer",
            Err("saved state was rejected".into()));
        assert!(unsafe_state.contains("solo recovery failed"));
        assert!(unsafe_state.contains("saved state was rejected"));
        assert!(unsafe_state.contains("Eject/relaunch"));
        assert!(!unsafe_state.contains("progress restored"));
    }

    #[test]
    fn multiplayer_bottom_legends_match_all_original_slot_screen_states() {
        use LinkLegendProfile as H;
        assert_eq!(link_legend_profile(LinkMenu::Pick(LinkRow::Host), true),
            H::PickerWithMode);
        assert_eq!(link_legend_profile(LinkMenu::Pick(LinkRow::Join), true),
            H::PickerWithMode);
        assert_eq!(link_legend_profile(LinkMenu::Pick(LinkRow::Host), false),
            H::Picker);
        assert_eq!(link_legend_profile(LinkMenu::Working {
            role: LinkRow::Host, step: LinkStep::Waiting, since: 100,
        }, true), H::Searching);
        assert_eq!(link_legend_profile(LinkMenu::Linked {
            role: LinkRow::Host, worked: 100, since: 500, opened: false,
        }, true), H::None);
        assert_eq!(link_legend_profile(LinkMenu::Linked {
            role: LinkRow::Join, worked: 100, since: 500, opened: true,
        }, true), H::ConnectedMenu);
        assert_eq!(link_legend_profile(LinkMenu::Failed {
            role: LinkRow::Join, fail: LinkFail::PeerVanished,
            worked: 100, since: 500,
        }, true), H::Failed);
        assert_eq!(link_legend_profile(LinkMenu::Unplug {
            role: LinkRow::Join, since: 500,
        }, true), H::None);
    }

    #[test]
    fn paused_game_controls_inherit_globals_until_overridden() {
        let mut settings=crate::settings::Settings::default();
        settings.ff_speed=6;
        settings.turbo=true; // KONKR original X/Y Buttons
        let defaults=LiveGameOptions::from_profile(&settings,None);
        assert_eq!(defaults.ff_speed,6);
        assert!(defaults.turbo);
        let mut profile=EmulationProfile::from_settings(&settings);
        assert!(profile.change_visual_and_gameplay(&settings,Platform::Gba,7,true));
        assert!(profile.change_visual_and_gameplay(&settings,Platform::Gba,4,true));
        let scoped=LiveGameOptions::from_profile(&settings,Some(profile));
        assert_ne!(scoped.ff_speed,defaults.ff_speed);
        assert_eq!(scoped.turbo,!defaults.turbo);
        assert_eq!(settings.ff_speed,6);
        assert!(settings.turbo,"global X/Y Buttons must not be modified");
        // A reset immediately restores the current Settings globals.
        assert_eq!(LiveGameOptions::from_profile(&settings,None).ff_speed,6);
    }

    #[test]
    fn eject_to_play_solo_is_an_action_not_a_noop() {
        assert_eq!(pause_rows(false,false),
            &[PauseRow::Resume,PauseRow::States,PauseRow::Multiplayer,
                PauseRow::Settings,PauseRow::SaveEject]);
        assert_eq!(pause_rows(true,false),
            &[PauseRow::Resume,PauseRow::Multiplayer,PauseRow::Settings,
                PauseRow::SaveEject]);
        assert_eq!(pause_rows(false,true),
            &[PauseRow::SaveEject,PauseRow::Multiplayer]);
        for (linked,needs_eject) in [(false,false),(true,false),(false,true),(true,true)] {
            for (index,item) in pause_rows(linked,needs_eject).iter().enumerate() {
                assert_eq!(game_menu_ejects(index,linked,needs_eject),
                    *item==PauseRow::SaveEject,
                    "never save/eject from States, Settings or Multiplayer");
            }
        }
        assert!(game_menu_ejects(0,true,true));
        assert!(!game_menu_ejects(0,true,false));
        assert_eq!(pause_row_text(PauseRow::SaveEject,true,true),78);
        assert_eq!(pause_row_text(PauseRow::Settings,true,false),25);
        assert!(!pause_rows(true,false).contains(&PauseRow::States));
    }

    #[test]
    fn slot_select_mode_only_allows_real_gpsp_serial_hardware_changes() {
        // Retail FireRed defaults to RFU. SELECT switches to real Cable
        // serial mode, which triggers a fresh gpSP boot before Join/Host.
        assert!(link_mode_can_switch(Core::Gpsp, LinkKind::Wireless,
            "BPRE", "POKEMON FIRE", true));
        assert!(link_mode_can_switch(Core::Gpsp, LinkKind::Cable,
            "BPRE", "POKEMON FIRE", true));
        assert_eq!(link_kind::serial_option(LinkKind::Wireless,
            LinkKind::Wireless, "BPRE", "POKEMON FIRE"), "auto");
        assert_eq!(link_kind::serial_option(LinkKind::Cable,
            LinkKind::Wireless, "BPRE", "POKEMON FIRE"), "mul_poke");
        assert!(!link_mode_can_switch(Core::Mgba, LinkKind::Cable,
            "BPRE", "POKEMON FIRE", true));
        assert!(!link_mode_can_switch(Core::Gpsp, LinkKind::Cable,
            "NONE", "UNKNOWN", false));
        assert!(!link_mode_can_switch(Core::Gpsp, LinkKind::Wireless,
            "BRBE", "MEGAMAN BN5", true),
            "gpSP RFU-only Battle Network must not offer a fake Cable mode");
    }

    #[test]
    fn link_failures_use_upstream_toasts_not_invented_link_sounds() {
        use crate::link_play::LinkError;
        assert_eq!(link_fault_toast(LinkError::BiosMismatch), Toast::BiosMismatch);
        for fault in [LinkError::PeerEnded, LinkError::Disconnected,
                      LinkError::InvalidState, LinkError::Timeout] {
            assert_eq!(link_fault_toast(fault), Toast::PeerEnded);
        }
        assert_eq!(Toast::LinkEnded.text(), "Link ended");
    }

    #[test]
    fn multiplayer_badges_match_original_host_join_broken_and_off() {
        assert_eq!(link_badge_for(LinkRow::Host, false), LinkBadge::Hosting);
        assert_eq!(link_badge_for(LinkRow::Join, false), LinkBadge::Joined);
        assert_eq!(link_badge_for(LinkRow::Host, true), LinkBadge::HostingLost);
        assert_eq!(link_badge_for(LinkRow::Join, true), LinkBadge::JoinedLost);
        for variant in LinkBadge::FACES {
            assert!(variant.badge().is_some());
            assert!(variant.colour().is_some());
            assert!(variant.face_index().is_some());
        }
        assert!(LinkBadge::Off.badge().is_none());
        assert!(LinkBadge::Off.face_index().is_none());
    }

    #[test]
    fn linked_reopen_stays_visible_and_unplug_uses_original_timing() {
        let first = LinkMenu::Linked {
            role: LinkRow::Host, worked: 100, since: 200, opened: false,
        };
        let reopened = LinkMenu::Linked {
            role: LinkRow::Host, worked: 100, since: 200, opened: true,
        };
        assert!(!link_menu_auto_closes(first, 1199));
        assert!(link_menu_auto_closes(first, 1200));
        assert!(!link_menu_auto_closes(reopened, 20000));
        let unplug = LinkMenu::Unplug { role: LinkRow::Join, since: 500 };
        assert!(!link_unplug_complete(unplug, 919));
        assert!(link_unplug_complete(unplug, 920));
        assert!(!link_unplug_complete(first, 10000));
    }

    #[test]
    fn connected_link_keycaps_fit_konkr_screen() {
        let linked_width = slot_ui::hint_width("B", "Back")
            + slot_ui::hint_width("A", "End Link");
        let failed_width = slot_ui::hint_width("A", "OK");
        for width in [linked_width, failed_width] {
            assert!((width as f32) + 15.0 < OUT_W as f32 - 60.0);
        }
    }

    #[test]
    fn multiplayer_b_restores_exactly_the_opening_context() {
        assert_eq!(LinkEntry::Shortcut.back(false,false), ShelfOverlay::None);
        assert_eq!(LinkEntry::GameMenu.back(false,false),
            ShelfOverlay::GameMenu { row: 2 });
        assert_eq!(LinkEntry::GameMenu.back(true,false),
            ShelfOverlay::GameMenu { row: 1 });
        assert_eq!(LinkEntry::GameMenu.back(true,true),
            ShelfOverlay::GameMenu { row: 1 });
    }

    #[test]
    fn select_home_opens_link_once_then_consumes_repeat_and_release() {
        let mut consumed = false;
        assert_eq!(select_home_action(true, true, true, true, &mut consumed),
            SelectHomeAction::OpenLink);
        assert!(consumed);
        // Android may issue HOME auto-repeat while SELECT is still held.
        assert_eq!(select_home_action(true, true, true, true, &mut consumed),
            SelectHomeAction::Swallow);
        assert_eq!(select_home_action(false, false, true, true, &mut consumed),
            SelectHomeAction::Swallow);
        assert!(!consumed);
        assert_eq!(select_home_action(false, false, true, true, &mut consumed),
            SelectHomeAction::Pass);
    }

    #[test]
    fn select_home_never_hijacks_shelf_menu_or_state_browser() {
        let mut consumed = false;
        for (select, playing, menu_allowed) in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            assert_eq!(select_home_action(true, select, playing, menu_allowed,
                &mut consumed), SelectHomeAction::Pass);
            assert!(!consumed);
        }
    }

    #[test]
    fn home_alone_keeps_original_quick_menu_and_long_eject_paths() {
        let mut consumed = false;
        assert_eq!(select_home_action(true, false, true, true, &mut consumed),
            SelectHomeAction::Pass);
        assert_eq!(select_home_action(false, false, true, true, &mut consumed),
            SelectHomeAction::Pass);
        assert!(!consumed);
    }

    #[test]
    fn background_library_swap_waits_for_idle_carousel() {
        assert!(may_apply_library_update(false, false, false));
        assert!(!may_apply_library_update(true, false, false));
        assert!(!may_apply_library_update(false, true, false));
        assert!(!may_apply_library_update(false, false, true));
    }

    #[test]
    fn rebuilding_an_empty_library_keeps_selection_safe() {
        let mut shelves: Vec<Shelf> = (0..3).map(|_| Shelf::new(Vec::new())).collect();
        assert_eq!(restore_shelf_focus(&mut shelves, 0, &[]), 0);
        assert_eq!(restore_shelf_focus(&mut shelves, 2, &[None, None, None]), 2);
    }

    #[test]
    fn fresh_avd_without_roms_has_no_selected_artwork_to_prefetch() {
        let shelves: Vec<Shelf> = (0..3).map(|_| Shelf::new(Vec::new())).collect();
        assert_eq!(selected_prefetch_entry(&shelves, 0), None);
        assert_eq!(selected_prefetch_entry(&shelves, 2), None);
        assert_eq!(selected_prefetch_entry(&shelves, 3), None);
    }

    #[test]
    fn in_game_shortcut_keycaps_fit_720x480_without_clipping() {
        let widths = [
            slot_ui::hint_width("B", "Back"),
            slot_ui::hint_width("SEL+R1", "Save"),
            slot_ui::hint_width("SEL+L1", "Load"),
        ];
        let visible = widths.iter()
            .map(|w| w.saturating_sub(HINT_EDGE) as f32)
            .sum::<f32>() + 2.0 * 15.0;
        assert!(visible < OUT_W as f32 - 2.0 * 30.0,
            "original keycap hints must stay visible on the KONKR 3:2 screen");
    }

    #[test]
    fn paused_game_menu_dpad_up_down_wrap_correctly() {
        assert_eq!(move_menu_row(0, 19, 3), 2);
        assert_eq!(move_menu_row(2, 20, 3), 0);
        assert_eq!(move_menu_row(1, 19, 3), 0);
        assert_eq!(move_menu_row(1, 20, 3), 2);
        assert_eq!(move_menu_row(1, 96, 3), 1);
    }

    #[test]
    fn original_settings_menu_rows_are_on_screen_and_outside_footer() {
        for rows in [3, 5, 6, 7] {
            let top = settings_row_top(rows);
            assert!(top >= 32.0);
            assert!(top + rows as f32 * QUICK_PITCH < 427.0,
                "settings text must not overlap Slot keycap legend");
            assert_eq!(top + (rows - 1) as f32 * QUICK_PITCH,
                settings_row_top(rows) + (rows - 1) as f32 * QUICK_PITCH);
        }
    }

    #[test]
    fn cartridge_timelines_match_upstream_seating_hold_and_ejection() {
        assert!((SCREEN_POWER_OFF_S - 0.16).abs() < 0.001);
        assert!((EJECT_HOLD_S - 0.35).abs() < 0.001);
        assert!((EJECT_S - 0.45).abs() < 0.001);
        assert!((SCREEN_POWER_OFF_S + EJECT_HOLD_S + EJECT_S - 0.96).abs() < 0.001);
        assert!(advance_cart(1.0, false, SCREEN_POWER_OFF_S) < 1.0);
        assert!(advance_cart(1.0, false, EJECT_S) < 0.0001);
        assert_eq!(advance_cart(0.0, true, SEATED_AT), 1.0);
        assert!((INSERT_S - SEATED_AT - 0.28).abs() < 0.001);
        // The original Slot lead is 97ms: sample impact should coincide
        // with the end of 450ms mechanical seating.
        assert!((SEATED_AT * INSERT_SOUND_PROGRESS + INSERT_SOUND_LEAD_S - SEATED_AT).abs() < 0.001);
        // The requested slightly later eject click must not alter mechanics.
        assert!((EJECT_S * (1.0 - EJECT_SOUND_PROGRESS) - 0.040).abs() < 0.001);
    }

    #[test]
    fn cartridge_sound_thresholds_follow_motion_without_starting_early() {
        // Insertion click is silent at 210ms, unlike the old Android port.
        let mut seat = advance_cart(0.0, true, 0.210);
        assert!(seat < INSERT_SOUND_PROGRESS);
        let previous = seat;
        seat = advance_cart(seat, true, 0.145);
        assert!(previous < INSERT_SOUND_PROGRESS && seat >= INSERT_SOUND_PROGRESS);

        // Ejection keeps the original black hold and waits a few frames
        // of actual motion before handing off the original PCM clip.
        let mut eject = 1.0;
        for _ in 0..2 {
            eject = advance_cart(eject, false, 1.0 / 60.0);
            assert!(eject > EJECT_SOUND_PROGRESS);
        }
        eject = advance_cart(eject, false, 1.0 / 60.0);
        assert!(eject <= EJECT_SOUND_PROGRESS);
    }

    #[test]
    fn ra_notice_bridge_removes_control_characters_and_bounds_payload() {
        let text = ra_notice_payload(&("X".repeat(200) + "\nsecret"), "Line\rTwo", true, None);
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(data["title"].as_str().unwrap().chars().count(), 100);
        assert!(!data["title"].as_str().unwrap().contains("secret"));
        assert_eq!(data["detail"], "LineTwo");
        assert_eq!(data["earned"], true);
        assert_eq!(data["badge"], "");
    }

    #[test]
    fn ra_notice_bridge_never_builds_an_unbounded_queue() {
        let mut notices = VecDeque::new();
        for index in 0..32 { queue_ra_notice(&mut notices, index.to_string()); }
        assert_eq!(notices.len(), RA_NOTICE_LIMIT);
        assert_eq!(notices.front().map(String::as_str), Some("24"));
        assert_eq!(notices.back().map(String::as_str), Some("31"));
    }

    #[test]
    fn select_chords_have_distinct_consumed_release_bits() {
        let mut combined = 0u16;
        for code in [96, 97, 100, 102, 103, 104, 105] {
            let bit = select_chord_bit(code);
            assert_ne!(bit, 0);
            assert_eq!(bit & combined, 0);
            combined |= bit;
        }
        assert_eq!(select_chord_bit(109), 0);
    }

    #[test]
    fn platform_name_timing_matches_upstream_slot() {
        // Regression: a zero-alpha first frame must not cancel the animation.
        assert!(platform_name_running(0));
        assert!(platform_name_running(1));
        assert!(platform_name_running(199));
        assert!(platform_name_running(2199));
        assert!(!platform_name_running(2200));
        assert!(!platform_name_running(3500));
        assert_eq!(slot_name_alpha(0), 0.0);
        assert!(slot_name_alpha(100) > 0.0);
        // v1.7.0 SLOT_NAME_ALPHA is 1.0, not the old Android 0.4.
        assert!((slot_name_alpha(200) - 1.0).abs() < 0.001);
        assert!((slot_name_alpha(1400) - 1.0).abs() < 0.001);
        assert!(slot_name_alpha(2000) > 0.0);
        assert_eq!(slot_name_alpha(2200), 0.0);
        assert_eq!(slot_name_alpha(3500), 0.0);
        assert_eq!(Platform::Gba.name(), "Game Boy Advance");
        assert_eq!(Platform::Gb.name(), "Game Boy");
        assert_eq!(Platform::Gbc.name(), "Game Boy Color");
        for platform in Platform::ALL {
            let face = platform_name_face(platform.name());
            assert_eq!(face.h, HINT_H * slot_ui::PLATFORM_NAME_SCALE);
            // Original names must fit the Slot canvas at native scale.
            assert!(face.w > 0 && face.w < 720 * slot_ui::PLATFORM_NAME_SCALE);
            assert_eq!(face.rgba.len(), (face.w * face.h * 4) as usize);
        }
    }

    #[test]
    fn empty_shelves_are_skipped_in_both_directions() {
        let mut shelves: Vec<Shelf> = (0..3).map(|_| Shelf::new(vec![])).collect();
        assert_eq!(next_nonempty_shelf(&shelves, 0, 1), 0);
        shelves[0].carts.push(Cart {
            platform: Platform::Gba, stem: "Test".into(), title: "Test".into(),
            code: String::new(), rom: PathBuf::new(), label: None, shell: None,
        });
        assert_eq!(next_nonempty_shelf(&shelves, 0, 1), 0);
        assert_eq!(next_nonempty_shelf(&shelves, 0, -1), 0);
        shelves[2].carts.push(Cart {
            platform: Platform::Gbc, stem: "Color".into(), title: "Color".into(),
            code: String::new(), rom: PathBuf::new(), label: None, shell: None,
        });
        assert_eq!(next_nonempty_shelf(&shelves, 0, 1), 2);
        assert_eq!(next_nonempty_shelf(&shelves, 0, -1), 2);
        assert_eq!(next_nonempty_shelf(&shelves, 2, 1), 0);
    }
}

// The primitive JNI ABI is sufficient for this stage; no Java objects are touched.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSurfaceCreated(
    _env: *mut c_void,
    _activity: *mut c_void,
) -> u8 {
    ENGINE.with(|holder| {
        let started = Instant::now();
        LIBRARY_UPDATE_SAFE.store(false, Ordering::Release);
        *holder.borrow_mut() = None;
        *RA_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        RA_NOTICES.lock().unwrap_or_else(|e| e.into_inner()).clear();
        PLAYING.store(false, Ordering::Release);
        CAROUSEL_QUIET.store(false, Ordering::Release);
        *WARM_ROM_URI.lock().unwrap_or_else(|e|e.into_inner()) = None;
        SAMPLE_RATE.store(0, Ordering::Release);
        let (version, snapshot) = {
            let state = LIBRARY.lock().unwrap_or_else(|err| err.into_inner());
            (state.version, state.entries.clone())
        };
        match Engine::new(version, snapshot.as_deref()) {
            Ok(engine) => {
                *holder.borrow_mut() = Some(engine);
                LIBRARY_UPDATE_SAFE.store(true, Ordering::Release);
                crate::game::log_launch_timing(&format!(
                    "Cold renderer ready in {}ms", started.elapsed().as_millis()));
                1
            }
            Err(error) => {
                eprintln!("slot-konkr: OpenGL renderer initialization failed: {error}");
                0
            }
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSurfaceChanged(
    _env: *mut c_void,
    _activity: *mut c_void,
    width: i32,
    height: i32,
) {
    ENGINE.with(|holder| {
        if let Some(engine) = holder.borrow_mut().as_mut() {
            engine.size = (width.max(1) as u32, height.max(1) as u32);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollCartLabelUri(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let uri = LABEL_PICK_REQUEST.lock().unwrap_or_else(|e| e.into_inner()).take();
    uri.and_then(|s| env.new_string(s).ok())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeReloadCartLabel(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>,
) {
    if let Ok(uri) = env.get_string(&uri) {
        INPUT.lock().unwrap_or_else(|e| e.into_inner())
            .push_back(Input::ReloadCartLabel {
                uri: uri.to_string_lossy().into_owned()
            });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeReloadVisualAssets(
    _env: *mut c_void,
    _activity: *mut c_void,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner()).push_back(Input::ReloadVisuals);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeDrawFrame(
    _env: *mut c_void,
    _activity: *mut c_void,
) {
    ENGINE.with(|holder| {
        let safe = holder.borrow().as_ref().is_some_and(|e|
            may_apply_library_update(e.game.is_some(), e.inserted, e.awaiting_game));
        LIBRARY_UPDATE_SAFE.store(safe, Ordering::Release);
        let current = holder.borrow().as_ref().map(|engine| engine.library_version);
        let updated = {
            let state = LIBRARY.lock().unwrap_or_else(|err| err.into_inner());
            if safe && current.is_some_and(|version| version != state.version) {
                Some((state.version, state.entries.clone()))
            } else {
                None
            }
        };
        if let Some((version, snapshot)) = updated {
            // Saving an in-progress game on library replacement is mandatory.
            if let Some(engine) = holder.borrow_mut().as_mut() {
                if let Some(mut session) = engine.game.take() { session.save(true); }
            }
            PLAYING.store(false, Ordering::Release);
            SAMPLE_RATE.store(0, Ordering::Release);
            // Preserve each system's selected URI before the Library update
            // destroys the old shelf. Cart Studio changes artwork, not focus.
            // On cold Android launch, the engine may be created before the
            // cached SAF library arrives. Do not overwrite the restored
            // on-disk U2 shelf position with that empty placeholder engine.
            let selected_uris = holder.borrow().as_ref()
                .filter(|engine|engine.shelves.iter().any(|s|!s.carts.is_empty()))
                .map(|engine| (
                    engine.active,
                    engine.shelves.iter().map(|shelf|
                        shelf.carts.get(shelf.index).map(|cart| cart.rom.clone())
                    ).collect::<Vec<_>>(),
                ));
            // Destruct old GL resources on the GL thread before recreating them.
            let size = holder.borrow().as_ref().map(|engine| engine.size).unwrap_or((960, 640));
            *holder.borrow_mut() = None;
            match Engine::new(version, snapshot.as_deref()) {
                Ok(mut engine) => {
                    engine.size = size;
                    if let Some((active, uris)) = selected_uris {
                        engine.active = restore_shelf_focus(&mut engine.shelves, active, &uris);
                    }
                    *holder.borrow_mut() = Some(engine);
                }
                Err(error) => eprintln!("slot-konkr: could not reload library: {error}"),
            }
        }
        if let Some(engine) = holder.borrow_mut().as_mut() { engine.draw(); }
        let now_safe = holder.borrow().as_ref().is_some_and(|e|
            may_apply_library_update(e.game.is_some(), e.inserted, e.awaiting_game));
        LIBRARY_UPDATE_SAFE.store(now_safe, Ordering::Release);
    });
}

/// The UI only stages library updates while the renderer is idle; the
/// renderer itself independently enforces that invariant before rebuilding.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeCanRefreshLibrary(
    _env: *mut c_void, _activity: *mut c_void,
) -> jboolean {
    if LIBRARY_UPDATE_SAFE.load(Ordering::Acquire) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeKey(
    _env: *mut c_void,
    _activity: *mut c_void,
    key: i32,
    pressed: u8,
) {
    INPUT
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push_back(Input::Key(key, pressed != 0));
}

/// Kotlin reports a compact, predefined library status. The renderer alone
/// owns the typography and START keycap; no Android TextView is overlaid.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetShelfStatus(
    _env: *mut c_void, _activity: *mut c_void, status: i32,
) {
    let safe = if (0..SHELF_STATUS_TEXT.len() as i32).contains(&status) {
        status as u8
    } else { 0 };
    INPUT.lock().unwrap_or_else(|poison| poison.into_inner())
        .push_back(Input::ShelfStatus(safe));
}

/// Kotlin resolves the connected Wi-Fi interface and takes a MulticastLock
/// before handing its IPv4 address to the independent Rust discovery worker.
/// Android's user-visible confirmation for sharing a complete ROM with
/// a trusted KMLB/4 mixed-cartridge peer. AGL never shows dialogs or blocks.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkPeerRomDecision(
    _env: *mut c_void, _activity: *mut c_void, choice: i32,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner())
        .push_back(Input::LinkPeerRomConsent(choice));
}

/// Safe short fingerprint derived from an authenticated Ed25519 public key;
/// no peer strings or ROM data reach the Android dialog.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkPeerLabel(
    env: JNIEnv<'_>, _activity: JObject<'_>,
) -> jstring {
    LINK_PEER_LABEL.lock().unwrap_or_else(|e| e.into_inner())
        .as_ref().and_then(|value| env.new_string(value).ok())
        .map_or(std::ptr::null_mut(), |value| value.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkNetworkReady(
    mut env: JNIEnv<'_>, _this: JObject<'_>, ipv4: JString<'_>,
) {
    if let Ok(value) = env.get_string(&ipv4) {
        INPUT.lock().unwrap_or_else(|e| e.into_inner())
            .push_back(Input::LinkNetworkReady(value.to_string_lossy().into_owned()));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkNetworkError(
    mut env: JNIEnv<'_>, _this: JObject<'_>, reason: JString<'_>,
) {
    if let Ok(value) = env.get_string(&reason) {
        INPUT.lock().unwrap_or_else(|e| e.into_inner())
            .push_back(Input::LinkNetworkError(value.to_string_lossy().into_owned()));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkNeedsMulticast(
    _env: *mut c_void, _this: *mut c_void,
) -> jboolean {
    LINK_NEEDS_MULTICAST.load(Ordering::Acquire) as jboolean
}

/// Called on the GL thread through GLSurfaceView.queueEvent before changing
/// account. The evaluator must drain its final queued frames, atomically store
/// all earned awards and confirm none remain only in RAM.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaFlushReady(
    _env: *mut c_void, _activity: *mut c_void,
) -> jboolean {
    ENGINE.with(|holder| {
        let engine = holder.borrow();
        let Some(engine) = engine.as_ref() else { return 1 };
        if engine.game.is_some() || engine.pending_game.is_some() { return 0; }
        match engine.ra_service.as_ref() {
            Some(service) if service.flush_ready() && !service.has_unsaved_awards() => 1,
            Some(_) => 0,
            None => 1,
        }
    })
}

/// Called only through GLSurfaceView.queueEvent AFTER the local flush barrier.
/// The service cannot be dropped until BOTH its evaluator and HTTP worker
/// acknowledge shutdown. Pending/badge/cache files remain untouched.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaStopReady(
    _env: *mut c_void, _activity: *mut c_void,
) -> jboolean {
    ENGINE.with(|holder| {
        let mut guard = holder.borrow_mut();
        let Some(engine) = guard.as_mut() else { return 1 };
        if engine.game.is_some() || engine.pending_game.is_some() { return 0 }
        engine.ra_signing_out = true;
        let ready = engine.ra_service.as_ref()
            .is_none_or(slot_achievements::Service::shutdown_ready);
        if ready {
            engine.ra_service.take();
            1
        } else { 0 }
    })
}

/// Account login or app startup: begin RA cache preparation and pending-award
/// retries even when no GBA cartridge has been inserted.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaAccountConfigured(
    _env: *mut c_void, _activity: *mut c_void,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner())
        .push_back(Input::RaAccountConfigured);
}

/// Full uncompressed GBA/GB/GBC ROM MD5 comes from Android SAF off the GL thread.
/// No ROM bytes or credentials are passed via this JNI boundary.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaPrefetchHash(
    mut env: JNIEnv<'_>, _this: JObject<'_>, hash: JString<'_>, console: jint,
) {
    if !matches!(console, 4 | 5 | 6) { return; }
    let Ok(hash) = env.get_string(&hash) else { return };
    let hash = hash.to_string_lossy().into_owned();
    if hash.len() != 32 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return;
    }
    let mut input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
    // Bound the GL input queue if Android is indexing thousands of files.
    if input.len() < 128 { input.push_back(Input::RaPrefetchHash(hash, console as u32)); }
}

/// Wake the RA HTTP worker on Android network reconnection. Do not perform
/// any HTTP, hashing or blocking operation on this callback thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaNetworkAvailable(
    _env: *mut c_void, _activity: *mut c_void,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner())
        .push_back(Input::RaNetworkAvailable);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeExternalReturn(
    _env: *mut c_void, _activity: *mut c_void,
) {
    push(Input::ExternalReturn);
}

/// Choreographer pacing experiment: this is only a render-request policy.
/// The underlying libretro emulation budget remains RESPONSIVE (one frame).
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeWarmRomCandidate(
    mut env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    WARM_ROM_URI.lock().unwrap_or_else(|e|e.into_inner())
        .clone()
        .and_then(|path|env.new_string(path).ok())
        .map_or(std::ptr::null_mut(),|value|value.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeCarouselQuiet(
    _env: *mut c_void, _activity: *mut c_void,
) -> u8 {
    u8::from(CAROUSEL_QUIET.load(Ordering::Acquire))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeDisplaySyncEnabled(
    _env: JNIEnv<'_>, _this: JObject<'_>,
) -> jboolean {
    if DISPLAY_SYNC.load(Ordering::Acquire) { 1 } else { 0 }
}

/// Bitmask: bit0 carousel idle, bit1 Wi-Fi, bit2 Bluetooth,
/// bit3 integrations. Never query the GL-only Engine from the UI thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeShelfIndicatorFlags(
    _env: *mut c_void, _this: *mut c_void,
) -> jint { SHELF_INDICATOR_FLAGS.load(Ordering::Acquire) }

/// Null when show-FPS is OFF, the game is not running or measurement isn't ready.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePerformanceStats(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    PERF_STATUS.lock().unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|text| env.new_string(text).ok())
        .map_or(std::ptr::null_mut(), |text| text.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeResetInput(
    _env: *mut c_void,
    _activity: *mut c_void,
) {
    let mut input = INPUT.lock().unwrap_or_else(|poison| poison.into_inner());
    input.clear();
    input.push_back(Input::Reset);
}


#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeCoreForUri(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>,
) -> jni::sys::jstring {
    let name = env.get_string(&uri).ok()
        .map(|uri| {
            let paths=PATHS.lock().unwrap_or_else(|e|e.into_inner()).clone();
            paths.map(|(storage,_)| core_selection::selected(&storage, &uri.to_string_lossy()))
                .unwrap_or(Core::Mgba).as_str()
        }).unwrap_or("mgba");
    env.new_string(name).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// Readable from the GL renderer without consulting Android preferences.
/// Android persists the opt-in first and then updates this display flag.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetShareSaves(
    _env: *mut c_void, _this: *mut c_void, enabled: jboolean,
) {
    SHARE_SAVES_ENABLED.store(enabled != 0, Ordering::Release);
}

/// Set only after Android's destination-core Save State dialogue has been
/// answered, and consume exactly once at the matching ROM load.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetNextResumeMode(
    _env: *mut c_void, _this: *mut c_void, mode: jint,
) {
    NEXT_RESUME_MODE.store(if matches!(mode, -1..=1) {mode} else {-1},
        Ordering::Release);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollSaveFlush(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jni::sys::jstring {
    let item=SAVE_SYNC.lock().unwrap_or_else(|e|e.into_inner()).pop_front();
    match item.and_then(|s|env.new_string(s).ok()) {
        Some(s)=>s.into_raw(),
        None=>std::ptr::null_mut(),
    }
}

// The Activity reads RA messages on its own main thread; it must not
// access the GL-thread-only ENGINE RefCell from Kotlin.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollRaNotice(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let notice = RA_NOTICES.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
    notice.and_then(|s| env.new_string(s).ok())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaStatus(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let state = RA_STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    state.and_then(|s| env.new_string(s).ok())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollUiAction(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    UI_ACTION.lock().unwrap_or_else(|e| e.into_inner()).pop_front().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollRumbleStrength(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    RUMBLE_STRENGTH.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollCartSfx(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    CART_SFX.lock().unwrap_or_else(|e| e.into_inner()).pop_front().unwrap_or(0)
}

fn push(input: Input) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner()).push_back(input);
}

/// Android provides the local clock and device battery to the original Slot footer.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSystemStatus(
    mut env: JNIEnv<'_>, _this: JObject<'_>, clock: JString<'_>,
    percent: jint, charging: jboolean,
) {
    let Ok(clock) = env.get_string(&clock) else { return; };
    let clock = clock.to_string_lossy().into_owned();
    if clock.len() > 16 || !clock.chars().all(|c| c.is_ascii_digit() || c == ':') { return; }
    let battery = if (0..=100).contains(&percent) {
        Some(Battery { percent: percent as u8,
            charge: if charging != 0 { Charge::Charging } else { Charge::Discharging } })
    } else { None };
    *SYSTEM_STATUS.lock().unwrap_or_else(|p| p.into_inner()) = SystemStatus { clock, battery };
}

/// App-private files directory and nativeLibraryDir (where mGBA is packaged).
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeConfigure(
    mut env: JNIEnv<'_>, _this: JObject<'_>, root: JString<'_>, libraries: JString<'_>,
) {
    if let (Ok(root), Ok(libraries)) = (env.get_string(&root), env.get_string(&libraries)) {
        *PATHS.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((PathBuf::from(root.to_string_lossy().as_ref()), PathBuf::from(libraries.to_string_lossy().as_ref())));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollLaunchUri(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let next = REQUEST.lock().unwrap_or_else(|e| e.into_inner()).take();
    match next.and_then(|v| env.new_string(v).ok()) {
        Some(v) => v.into_raw(), None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeGameReady(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>, path: JString<'_>,
) {
    if let (Ok(uri), Ok(path)) = (env.get_string(&uri), env.get_string(&path)) {
        push(Input::GameReady {
            uri: uri.to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
        });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeGameError(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>, message: JString<'_>,
) {
    if let (Ok(uri), Ok(message)) = (env.get_string(&uri), env.get_string(&message)) {
        push(Input::GameError {
            uri: uri.to_string_lossy().into_owned(),
            message: message.to_string_lossy().into_owned(),
        });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeExitGame(
    _env: *mut c_void, _this: *mut c_void,
) { push(Input::Exit); }

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSuspend(
    _env: *mut c_void, _this: *mut c_void,
) {
    ENGINE.with(|holder| {
        if let Some(engine) = holder.borrow_mut().as_mut() { engine.handle(Input::Suspend); }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeIsPlaying(
    _env: *mut c_void, _this: *mut c_void,
) -> jboolean { PLAYING.load(Ordering::Acquire) as jboolean }

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeAudioSpeedPermille(
    _env: *mut c_void, _this: *mut c_void,
) -> jint { AUDIO_SPEED_PERMILLE.load(Ordering::Acquire) }

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeFastAudioSupported(
    _env: *mut c_void, _this: *mut c_void, supported: jboolean,
) {
    FF_AUDIO_SUPPORTED.store(supported != 0, Ordering::Release);
    if supported == 0 { AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear(); }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetMgbaLinkRecoveryEnabled(
    _env: *mut c_void, _this: *mut c_void, enabled: jboolean,
) {
    MGBA_CABLE_RECOVERY.store(enabled != 0, Ordering::Release);
    crate::game::log_launch_timing(if enabled != 0 {
        "A12 Link recovery setting: a11"
    } else {
        "A12 Link recovery setting: legacy"
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeAudioQueuedShorts(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    AUDIO.lock().unwrap_or_else(|e| e.into_inner())
        .len().min(i32::MAX as usize) as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeAudioSampleRate(
    _env: *mut c_void, _this: *mut c_void,
) -> jint { SAMPLE_RATE.load(Ordering::Acquire) }

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeReadAudio(
    mut env: JNIEnv<'_>, _this: JObject<'_>,
) -> jshortArray {
    let samples: Vec<i16> = {
        let mut audio = AUDIO.lock().unwrap_or_else(|e| e.into_inner());
        let n = audio.len().min(4096);
        audio.drain(..n).collect()
    };
    let Ok(out) = env.new_short_array(samples.len() as i32) else { return std::ptr::null_mut(); };
    let _ = env.set_short_array_region(&out, 0, &samples);
    out.into_raw()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollMessage(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    match MESSAGE.lock().unwrap_or_else(|e| e.into_inner()).take().and_then(|v| env.new_string(v).ok()) {
        Some(v) => v.into_raw(), None => std::ptr::null_mut(),
    }
}
