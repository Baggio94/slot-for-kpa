//! Per-cartridge opt-in latency profiles. Never touch the ROM or RetroArch saves.
use std::{fs, io, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::{core_selection, settings::{self, Settings, Shader, Bezel}};
use slot_store::{Core, Platform};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmulationProfile {
    pub show_fps: bool,
    // None means a legacy per-game profile inherits global Show Stats.
    // A newly edited profile persists its own explicit ON/OFF selection.
    pub show_fps_override: Option<bool>,
    // Frame Pacing RESPONSIVE (Choreographer loop) instead of NATIVE.
    pub responsive_loop: bool,
    pub run_ahead_frames: u8,
    // U1 pause menus: all are optional and inherit the corresponding
    // global preference until this specific ROM+core is edited.
    pub screen_shader: Option<Shader>,
    pub screen_bezel: Option<Bezel>,
    pub colour_correction_override: Option<bool>,
    pub gb_palettes_override: Option<bool>,
    pub ff_speed_override: Option<u8>,
    pub ff_sound_override: Option<bool>,
    pub rewind_override: Option<bool>,
    pub rewind_speed_override: Option<u8>,
    pub turbo_override: Option<bool>, // original KONKR "X/Y Buttons"
    pub rumble_override: Option<bool>,
    pub eject_save_override: Option<bool>,
    // Boktai's sun: 0..=10, SUN_AUTO for the time of day, or SUN_SENSOR
    // for the light sensor. None follows the handheld (sensor if it has
    // one).
    pub sun: Option<u8>,
}

pub const SUN_SENSOR: u8 = 11;
pub const SUN_AUTO: u8 = 12;
/// Local time of day in minutes, from Android's status clock; u32::MAX
/// until the first one arrives.
pub static LOCAL_MINUTES: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(u32::MAX);
/// "HH:MM" from the status clock, in minutes since midnight.
pub fn set_local_clock(clock: &str) {
    let mut parts = clock.split(':').map(|part| part.parse::<u32>().ok());
    if let (Some(Some(hours)), Some(Some(minutes)), None) =
        (parts.next(), parts.next(), parts.next()) {
        if hours < 24 && minutes < 60 {
            LOCAL_MINUTES.store(hours * 60 + minutes, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
/// AUTO: the sun follows the clock, as the real cartridge follows the
/// sky. Night until 6:00, full at 13:00, night again from 20:00. Without
/// a clock yet, half the gauge.
pub fn auto_sun(minutes: u32, top: u8) -> u8 {
    const RISE: f32 = 6.0 * 60.0;
    const SET: f32 = 20.0 * 60.0;
    if minutes == u32::MAX { return top.div_ceil(2); }
    let minutes = minutes as f32;
    if minutes <= RISE || minutes >= SET { return 0; }
    let day = std::f32::consts::PI * (minutes - RISE) / (SET - RISE);
    (f32::from(top) * day.sin()).round() as u8
}
/// Night, like RetroArch, or the real light when the handheld can
/// measure it. Each Boktai calibrates its sensor in the dark once and
/// counts its bars from there: calibrated under a manual sun, its gauge
/// never fills.
pub fn default_sun(light_sensor: bool) -> u8 {
    if light_sensor { SUN_SENSOR } else { 0 }
}
/// The highest manual sun: one per bar of the game's gauge, 8 in
/// Boktai 1 and 10 in Boktai 2 and 3.
pub fn sun_top(boktai1: bool) -> u8 {
    if boktai1 { 8 } else { 10 }
}
/// The light a solar cartridge reads for a manual sun, None for SENSOR.
/// Sun N shows N bars, measured on a KPA with the game calibrated at 0.
/// Boktai 1 (8 bars) uses mGBA's own steps (0x16 + GBA_LUX_LEVELS),
/// leaving out the two that repeated a bar (106 and 161). Boktai 2 (10
/// bars) showed 2 bars at 33, 3 at 39 and 45, 4 at 56, 5 at 68, 6 at 80,
/// 7 from 91 (flickering with 6 right there), 8 at 114, 9 at 137 and 10
/// from 183; 1 lies below 33.
pub fn sun_luminance(sun: u8, boktai1: bool) -> Option<u8> {
    const BOKTAI1: [u8; 9] = [22, 27, 33, 40, 49, 64, 84, 131, 205];
    const LATER: [u8; 11] = [22, 27, 33, 45, 56, 68, 80, 96, 114, 137, 183];
    if sun == SUN_SENSOR { return None; }
    let top = sun_top(boktai1);
    let sun = if sun == SUN_AUTO {
        auto_sun(LOCAL_MINUTES.load(std::sync::atomic::Ordering::Relaxed), top)
    } else { sun };
    let sun = usize::from(sun.min(top));
    Some(if boktai1 { BOKTAI1[sun] } else { LATER[sun] })
}
/// The brightest light a game accepts: from 254, Boktai 2 reads the
/// cartridge's answer as no sun at all and its gauge drains to 0.
pub const MAX_LUMINANCE: u8 = 253;
/// mGBA's own conversion of light sensor lux (cbrt(lux) * 8).
pub fn lux_luminance(lux: f32) -> u8 {
    (lux.max(0.0).cbrt() * 8.0).min(f32::from(MAX_LUMINANCE)) as u8
}

impl EmulationProfile {
    pub fn from_settings(settings: &Settings) -> Self {
        Self { show_fps: settings.show_fps,
            show_fps_override: None,
            responsive_loop: settings.responsive_loop,
            run_ahead_frames: settings.run_ahead_frames,
            screen_shader: None, screen_bezel: None,
            colour_correction_override: None,
            gb_palettes_override: None, ff_speed_override: None,
            ff_sound_override: None, rewind_override: None,
            rewind_speed_override: None,
            turbo_override: None, rumble_override: None,
            eject_save_override: None, sun: None }
    }
    /// Materialise the *current* game's per-ROM settings without mutating
    /// Settings.json or any other game's configured defaults.
    pub fn apply_visual_and_gameplay(&self, settings: &mut Settings, platform: Platform) {
        if let Some(shader)=self.screen_shader {
            // Non-destructive migration for GBA per-game profiles saved before
            // the official LCD Grid V2 replaced Slot's old Grid entry.
            let shader=if platform==Platform::Gba && shader==Shader::Grid {
                Shader::LcdGridV2
            } else { shader };
            settings.set_shader_for(platform,shader);
        }
        if let Some(value)=self.colour_correction_override {
            settings.set_colour_for(platform,value);
        }
        if let Some(value)=self.screen_bezel {settings.set_bezel_for(platform,value);}
        if let Some(value)=self.gb_palettes_override { settings.gb_palettes=value; }
        if let Some(value)=self.ff_speed_override { settings.ff_speed=value; }
        if let Some(value)=self.ff_sound_override { settings.ff_sound=value; }
        if let Some(value)=self.rewind_override { settings.rewind=value; }
        if let Some(speed)=self.rewind_speed_override { settings.rewind_speed=speed; }
        if let Some(value)=self.turbo_override { settings.turbo=value; }
        if let Some(value)=self.rumble_override { settings.rumble=value; }
        if let Some(value)=self.eject_save_override { settings.eject_save=value; }
    }

    /// Screen/Gameplay pause edits are scoped to the current cart and core.
    /// Using the existing global Settings::change() preserves Slot's exact
    /// cycling rules, including its KONKR X/Y Buttons naming/values.
    pub fn change_visual_and_gameplay(&mut self, global: &Settings,
        platform: Platform, key: usize, right: bool) -> bool
    {
        let mut view=global.clone();
        self.apply_visual_and_gameplay(&mut view,platform);
        // Menu key 1 inherited shared GB/GBC shader from RC1. Route
        // GBC to its new key without altering the GBA or GB settings.
        let actual_key=match (platform,key) {
            (Platform::Gbc,1)=>21,
            (Platform::Gba,2)=>22,
            (Platform::Gbc,2)=>23,
            (Platform::Gb,2)=>24,
            (_,25..=27)=>match platform {
                Platform::Gba=>25,Platform::Gbc=>26,Platform::Gb=>27
            },
            _=>key,
        };
        if !view.change(actual_key,right) { return false; }
        match key {
            0 if platform==Platform::Gba =>
                self.screen_shader=Some(view.shader_gba),
            1 if platform!=Platform::Gba =>
                self.screen_shader=Some(view.shader_for(platform)),
            2 => self.colour_correction_override=Some(view.colour_for(platform)),
            25..=27 => self.screen_bezel=Some(view.bezel_for(platform)),
            3 if platform==Platform::Gb =>
                self.gb_palettes_override=Some(view.gb_palettes),
            4 => self.ff_speed_override=Some(view.ff_speed),
            5 => self.ff_sound_override=Some(view.ff_sound),
            6 => {
                self.rewind_override=Some(view.rewind);
                self.rewind_speed_override=Some(view.rewind_speed);
            },
            7 => self.turbo_override=Some(view.turbo),
            8 => self.rumble_override=Some(view.rumble),
            9 => self.eject_save_override=Some(view.eject_save),
            _ => return false,
        }
        true
    }

    pub fn sanitize(mut self) -> Self {
        if self.run_ahead_frames > 2 { self.run_ahead_frames = 0; }
        if self.rewind_speed_override.is_some_and(|speed|
            !settings::REWIND_SPEEDS.contains(&speed)) {
            self.rewind_speed_override=None;
        }
        if !settings::SPEEDS.contains(&self.ff_speed_override.unwrap_or(2)) {
            self.ff_speed_override=None;
        }
        if self.sun.is_some_and(|sun| sun > SUN_AUTO) { self.sun = None; }
        self
    }
    pub fn change(&mut self, key: usize, right: bool) -> bool {
        let before = *self;
        match key {
            10 => {
                self.show_fps = !self.show_fps;
                self.show_fps_override = Some(self.show_fps);
            },
            11 => self.responsive_loop = !self.responsive_loop,
            12 => self.run_ahead_frames =
                settings::cycle_u8(self.run_ahead_frames, 2, right),
            _ => return false,
        }
        *self != before
    }
    /// The game's sun, `top` being its highest manual level.
    pub fn sun(&self, light_sensor: bool, top: u8) -> u8 {
        match self.sun {
            Some(SUN_SENSOR) if !light_sensor => default_sun(false),
            Some(SUN_SENSOR) => SUN_SENSOR,
            Some(SUN_AUTO) => SUN_AUTO,
            Some(sun) => sun.min(top),
            None => default_sun(light_sensor),
        }
    }
    /// 0, 1 … top, AUTO, then SENSOR when the handheld has a light sensor.
    pub fn change_sun(&mut self, light_sensor: bool, top: u8, right: bool) {
        let at = match self.sun(light_sensor, top) {
            SUN_AUTO => top + 1,
            SUN_SENSOR => top + 2,
            sun => sun,
        };
        let next = settings::cycle_u8(at, top + 1 + light_sensor as u8, right);
        self.sun = Some(match next - next.min(top) {
            0 => next,
            1 => SUN_AUTO,
            _ => SUN_SENSOR,
        });
    }
    fn legacy_path(root: &Path, uri: &str) -> PathBuf {
        root.join("GameEmulation").join(format!("{}.json", core_selection::key(uri)))
    }
    fn path(root: &Path, uri: &str, core: Core) -> PathBuf {
        root.join("GameEmulation").join(
            format!("{}.{}.json", core_selection::key(uri), core.as_str()))
    }
    /// Core-specific profiles supersede old ROM-only JSON files. Reading an
    /// old profile never migrates or deletes it: the other core can still
    /// inherit it until the user customizes or resets that core explicitly.
    pub fn load(root: &Path, uri: &str, core: Core) -> Option<Self> {
        let value: serde_json::Value = match fs::read(Self::path(root, uri, core)) {
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                // A reset must mask legacy ROM-only overrides for this core.
                if value.get("inherit_global").and_then(|v| v.as_bool()) == Some(true) {
                    return None;
                }
                value
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound =>
                serde_json::from_slice(&fs::read(Self::legacy_path(root, uri)).ok()?).ok()?,
            Err(_) => return None,
        };
        let native = settings::legacy_pacing_is_native(&value);
        serde_json::from_value::<Self>(value).ok().map(|mut profile| {
            if native { profile.responsive_loop = false; }
            profile.sanitize()
        })
    }
    fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
        fs::create_dir_all(path.parent().expect("app-owned profile path"))?;
        // fsync before and after the rename: a power cut keeps either the
        // previous profile or the new one, never an empty file.
        slot_store::atomic_write(path, data)
    }
    pub fn save(self, root: &Path, uri: &str, core: Core) -> io::Result<()> {
        let data = serde_json::to_vec_pretty(&self.sanitize()).map_err(io::Error::other)?;
        Self::atomic_write(&Self::path(root, uri, core), &data)
    }
    /// Reset only the selected core. A tiny marker masks an earlier per-ROM
    /// profile without rewriting or deleting that legacy file (or the other
    /// core's profile). With no legacy profile, removal restores inheritance.
    pub fn clear(root: &Path, uri: &str, core: Core) -> io::Result<()> {
        let path = Self::path(root, uri, core);
        if Self::legacy_path(root, uri).exists() {
            Self::atomic_write(&path, br#"{"inherit_global":true}"#)
        } else {
            match fs::remove_file(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sun_follows_the_handheld_until_chosen() {
        let mut profile=EmulationProfile::default();
        assert_eq!(profile.sun(true, 10), SUN_SENSOR);
        assert_eq!(profile.sun(false, 10), 0, "night, to calibrate in the dark");
        profile.change_sun(true, 10, true);
        assert_eq!(profile.sun(true, 10), 0, "after SENSOR comes 0");
        profile.change_sun(false, 10, false);
        assert_eq!(profile.sun(false, 10), SUN_AUTO, "AUTO closes the round");
        profile.change_sun(false, 10, false);
        assert_eq!(profile.sun(false, 10), 10);
        assert_eq!(profile.sun(false, 8), 8, "Boktai 1 stops at 8 bars");
        profile.change_sun(true, 8, true);
        assert_eq!(profile.sun(true, 8), SUN_AUTO, "AUTO follows 8 in Boktai 1");
        profile.change_sun(true, 8, true);
        assert_eq!(profile.sun(true, 8), SUN_SENSOR, "then SENSOR");
        profile.sun = Some(SUN_AUTO);
        profile.change_sun(false, 8, true);
        assert_eq!(profile.sun(false, 8), 0, "no SENSOR without a light sensor");
        assert_eq!(EmulationProfile { sun: Some(SUN_AUTO), ..profile }.sanitize().sun,
            Some(SUN_AUTO));
        profile.sun=Some(SUN_SENSOR);
        assert_eq!(profile.sun(false, 10), 0, "no light sensor on this handheld");
        assert_eq!(sun_luminance(SUN_SENSOR, true), None);
        assert_eq!(sun_luminance(0, true), Some(0x16));
        assert_eq!(sun_luminance(7, true), Some(0x16 + 109));
        assert_eq!(sun_luminance(8, true), Some(0x16 + 183));
        assert_eq!(sun_luminance(10, true), Some(0x16 + 183));
        assert_eq!(sun_luminance(0, false), Some(22));
        assert_eq!(sun_luminance(5, false), Some(68));
        assert_eq!(sun_luminance(10, false), Some(183));
        assert_eq!(lux_luminance(1000.0), 80);
        assert_eq!(lux_luminance(1e9), MAX_LUMINANCE);
        assert_eq!(EmulationProfile { sun: Some(40), ..profile }.sanitize().sun, None);
    }
    #[test]
    fn auto_sun_follows_the_clock() {
        let at = |h: u32, m: u32| auto_sun(h * 60 + m, 10);
        assert_eq!(at(3, 0), 0);
        assert_eq!(at(6, 0), 0);
        assert_eq!(at(9, 30), 7);
        assert_eq!(at(13, 0), 10, "full at 13:00");
        assert_eq!(at(16, 30), 7);
        assert_eq!(at(20, 0), 0);
        assert_eq!(auto_sun(13 * 60, 8), 8);
        assert_eq!(auto_sun(u32::MAX, 10), 5, "half the gauge without a clock");
        set_local_clock("bad");
        set_local_clock("25:00");
        assert_eq!(LOCAL_MINUTES.load(std::sync::atomic::Ordering::Relaxed), u32::MAX);
        set_local_clock("13:00");
        assert_eq!(sun_luminance(SUN_AUTO, false), Some(183));
        assert_eq!(sun_luminance(SUN_AUTO, true), Some(0x16 + 183));
    }
    #[test]
    fn old_per_game_gba_grid_becomes_grid_v2_without_changing_gb() {
        let profile=EmulationProfile {
            screen_shader: Some(Shader::Grid), ..Default::default()
        };
        let mut gba=Settings::default();
        profile.apply_visual_and_gameplay(&mut gba,Platform::Gba);
        assert_eq!(gba.shader_gba,Shader::LcdGridV2);
        let mut gb=Settings::default();
        profile.apply_visual_and_gameplay(&mut gb,Platform::Gb);
        assert_eq!(gb.shader_gb,Shader::Grid);
    }

    #[test]
    fn legacy_globals_inherited_until_game_is_customized() {
        let mut settings = Settings::default();
        settings.responsive_loop = true;
        let mut profile = EmulationProfile::from_settings(&settings);
        assert!(profile.responsive_loop);
        assert!(profile.change(12, true));
        assert_eq!(profile.run_ahead_frames, 1);
        assert!(profile.change(12, true));
        assert_eq!(profile.run_ahead_frames, 2);
        assert!(profile.change(12, true));
        assert_eq!(profile.run_ahead_frames, 0);
        assert_eq!(EmulationProfile::default().show_fps_override, None);
    }

    #[test]
    fn old_per_game_pacing_modes_become_native() {
        let root = std::env::temp_dir().join(format!(
            "slot-konkr-emulation-pacing-{}", std::process::id()));
        let uri = "content://roms/pacing";
        let path = EmulationProfile::path(&root, uri, Core::Mgba);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for old in [r#"{"responsive_pacing":true,"run_ahead_frames":1}"#,
            r#"{"responsive_pacing":true,"display_sync_pacing":true}"#,
            r#"{"hybrid_pacing":true}"#, r#"{"retroarch_clock_pacing":true}"#,
            r#"{"retroarch_sync_pacing":false,"responsive_loop":true}"#] {
            fs::write(&path, old).unwrap();
            let p = EmulationProfile::load(&root, uri, Core::Mgba).unwrap();
            assert!(!p.responsive_loop, "{old}");
        }
        fs::write(&path,
            r#"{"retroarch_sync_pacing":true,"responsive_loop":true,"run_ahead_frames":1}"#)
            .unwrap();
        let kept = EmulationProfile::load(&root, uri, Core::Mgba).unwrap();
        assert!(kept.responsive_loop && kept.run_ahead_frames == 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn per_game_frame_pacing_toggles_and_roundtrips_without_global_changes() {
        let globals = Settings::first_install_defaults();
        let mut profile = EmulationProfile::from_settings(&globals);
        assert!(!profile.responsive_loop);
        assert!(profile.change(11, true));
        assert!(profile.responsive_loop);
        let json = serde_json::to_string(&profile).unwrap();
        let restored: EmulationProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, profile);
        assert!(!globals.responsive_loop);
        assert!(profile.change(11, false) && !profile.responsive_loop);
    }

    #[test]
    fn new_game_profiles_inherit_native_pacing_and_no_runahead() {
        let fresh = Settings::first_install_defaults();
        let game = EmulationProfile::from_settings(&fresh);
        assert!(!game.responsive_loop);
        assert_eq!(game.run_ahead_frames, 0);
        let mut responsive = fresh.clone();
        responsive.responsive_loop = true;
        responsive.run_ahead_frames = 1;
        let game = EmulationProfile::from_settings(&responsive);
        assert!(game.responsive_loop && game.run_ahead_frames == 1);
    }

    #[test]
    fn old_game_profiles_inherit_global_stats_until_explicitly_changed() {
        let old: EmulationProfile = serde_json::from_str(
            r#"{"show_fps":false,"display_sync_pacing":true,"responsive_pacing":true}"#
        ).unwrap();
        assert_eq!(old.show_fps_override, None);
        assert!(!old.show_fps);
        let mut profile = EmulationProfile::from_settings(&Settings::default());
        assert_eq!(profile.show_fps_override, None);
        assert!(profile.change(10, true));
        assert_eq!(profile.show_fps_override, Some(true));
        assert!(profile.change(10, true));
        assert_eq!(profile.show_fps_override, Some(false));
    }

    #[test]
    fn rewind_speed_inherits_global_until_per_game_is_customized() {
        let mut global=Settings::default();
        global.rewind_speed=3;
        let mut profile=EmulationProfile::from_settings(&global);
        let legacy: EmulationProfile=serde_json::from_str(
            r#"{"rewind_override":true,"ff_speed_override":4}"#).unwrap();
        assert_eq!(legacy.rewind_speed_override,None);
        let mut scoped=global.clone();
        legacy.apply_visual_and_gameplay(&mut scoped,Platform::Gba);
        assert_eq!(scoped.rewind_speed,3);
        assert!(profile.change_visual_and_gameplay(&global,Platform::Gba,6,true));
        assert_eq!(profile.rewind_override,Some(true));
        assert_eq!(profile.rewind_speed_override,Some(5));
        assert_eq!(global.rewind_speed,3);
        let mut scoped=global.clone();
        profile.apply_visual_and_gameplay(&mut scoped,Platform::Gba);
        assert_eq!(scoped.rewind_speed,5);
        assert_eq!(scoped.ff_speed,global.ff_speed);
        assert_eq!(scoped.run_ahead_frames,global.run_ahead_frames);
        let json=serde_json::to_string(&profile).unwrap();
        assert_eq!(serde_json::from_str::<EmulationProfile>(&json).unwrap(),profile);
    }

    #[test]
    fn pause_screen_and_gameplay_edits_never_change_global_defaults() {
        let baseline=Settings::default();
        let global=baseline.clone();
        let mut p=EmulationProfile::from_settings(&global);
        assert!(p.change_visual_and_gameplay(&global,Platform::Gba,0,true));
        assert!(p.change_visual_and_gameplay(&global,Platform::Gba,2,true));
        assert!(p.change_visual_and_gameplay(&global,Platform::Gba,7,true));
        assert_eq!(baseline,global,"Pause edits never mutate Settings.json");
        assert_eq!(p.screen_shader,Some(Shader::Lcd3x));
        assert_eq!(p.colour_correction_override,Some(true));
        assert_eq!(p.turbo_override,Some(!global.turbo));
        let mut effective=global.clone();
        p.apply_visual_and_gameplay(&mut effective,Platform::Gba);
        assert_eq!(effective.shader_gba,Shader::Lcd3x);
        assert_eq!(effective.shader_gb,global.shader_gb);
        assert!(!effective.turbo);
        assert!(effective.colour_correction);
    }

    #[test]
    fn gbc_screen_override_does_not_touch_gb_or_gba_defaults() {
        let global=Settings::default();
        let mut profile=EmulationProfile::from_settings(&global);
        assert!(profile.change_visual_and_gameplay(&global,Platform::Gbc,1,true));
        assert!(profile.change_visual_and_gameplay(&global,Platform::Gbc,2,true));
        assert!(profile.change_visual_and_gameplay(&global,Platform::Gbc,26,true));
        let mut scoped=global.clone();
        profile.apply_visual_and_gameplay(&mut scoped,Platform::Gbc);
        assert_ne!(scoped.shader_gbc,global.shader_gbc);
        assert_eq!(scoped.shader_gb,global.shader_gb);
        assert_eq!(scoped.shader_gba,global.shader_gba);
        assert!(scoped.colour_correction_gbc);
        assert_eq!(scoped.colour_correction_gb,global.colour_correction_gb);
        assert_eq!(scoped.bezel_gbc,Bezel::Color);
        assert_eq!(global,Settings::default());
    }

    #[test]
    fn new_per_game_visual_overrides_are_absent_in_all_legacy_json() {
        let p:EmulationProfile=serde_json::from_str(
            r#"{"responsive_pacing":true,"run_ahead_frames":1}"#).unwrap();
        assert_eq!(p.screen_shader,None);
        assert_eq!(p.ff_speed_override,None);
        assert_eq!(p.turbo_override,None);
        let mut s=Settings::default();
        p.apply_visual_and_gameplay(&mut s,Platform::Gba);
        assert_eq!(s,Settings::default());
    }

    #[test]
    fn profiles_are_isolated_by_cartridge_uri_and_reset_cleanly() {
        let root = std::env::temp_dir().join(format!(
            "slot-konkr-emulation-{}", std::process::id()));
        let a = "content://game/A";
        let b = "content://game/B";
        let mut profile = EmulationProfile::default();
        profile.run_ahead_frames = 1;
        profile.show_fps = true;
        profile.show_fps_override = Some(true);
        profile.save(&root, a, Core::Mgba).unwrap();
        assert_eq!(EmulationProfile::load(&root, a, Core::Mgba), Some(profile));
        assert_eq!(EmulationProfile::load(&root, b, Core::Mgba), None);
        EmulationProfile::clear(&root, a, Core::Mgba).unwrap();
        assert_eq!(EmulationProfile::load(&root, a, Core::Mgba), None);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn legacy_json_survives_core_isolation_and_explicit_resets() {
        let root = std::env::temp_dir().join(format!(
            "slot-konkr-emulation-legacy-{}", std::process::id()));
        let uri = "content://roms/advance-wars-2";
        let legacy = EmulationProfile::legacy_path(&root, uri);
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let bytes = br#"{"responsive_pacing":true,"display_sync_pacing":true,"run_ahead_frames":1}"#;
        fs::write(&legacy, bytes).unwrap();
        let inherited = EmulationProfile::load(&root, uri, Core::Mgba).unwrap();
        assert_eq!(EmulationProfile::load(&root, uri, Core::Gpsp), Some(inherited));
        assert_eq!(inherited.show_fps_override, None);

        let mut mgba = inherited;
        mgba.run_ahead_frames = 2;
        mgba.show_fps_override = Some(false);
        mgba.save(&root, uri, Core::Mgba).unwrap();
        assert_eq!(EmulationProfile::load(&root, uri, Core::Mgba), Some(mgba));
        assert_eq!(EmulationProfile::load(&root, uri, Core::Gpsp), Some(inherited));

        EmulationProfile::clear(&root, uri, Core::Mgba).unwrap();
        assert_eq!(EmulationProfile::load(&root, uri, Core::Mgba), None);
        assert_eq!(EmulationProfile::load(&root, uri, Core::Gpsp), Some(inherited));
        EmulationProfile::clear(&root, uri, Core::Gpsp).unwrap();
        assert_eq!(EmulationProfile::load(&root, uri, Core::Gpsp), None);
        assert_eq!(fs::read(&legacy).unwrap().as_slice(), bytes);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profiles_for_each_core_and_each_rom_do_not_collide() {
        let root = std::env::temp_dir().join(format!(
            "slot-konkr-emulation-scope-{}", std::process::id()));
        let uri = "content://roms/virtua-tennis";
        let other = "content://roms/advance-wars-2";
        let mut one = EmulationProfile::default();
        one.run_ahead_frames = 1;
        let mut two = one;
        two.run_ahead_frames = 2;
        one.save(&root, uri, Core::Mgba).unwrap();
        two.save(&root, uri, Core::Gpsp).unwrap();
        assert_eq!(EmulationProfile::load(&root, uri, Core::Mgba), Some(one));
        assert_eq!(EmulationProfile::load(&root, uri, Core::Gpsp), Some(two));
        assert_eq!(EmulationProfile::load(&root, other, Core::Mgba), None);
        EmulationProfile::clear(&root, uri, Core::Gpsp).unwrap();
        assert_eq!(EmulationProfile::load(&root, uri, Core::Mgba), Some(one));
        assert_eq!(EmulationProfile::load(&root, uri, Core::Gpsp), None);
        let _ = fs::remove_dir_all(root);
    }

}
