//! Per-cartridge opt-in latency profiles. Never touch the ROM or RetroArch saves.
use std::{fs, io, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::{core_selection, settings::{self, Settings, Shader, Bezel}};
use slot_store::{Core, Platform};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmulationProfile {
    pub show_fps: bool,
    // None means a legacy per-game profile inherits global Show Stats.
    // A newly edited profile persists its own explicit ON/OFF selection.
    pub show_fps_override: Option<bool>,
    pub responsive_pacing: bool,
    pub display_sync_pacing: bool,
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
    pub turbo_override: Option<bool>, // original KONKR "X/Y Buttons"
    pub rumble_override: Option<bool>,
    pub eject_save_override: Option<bool>,
}
impl Default for EmulationProfile {
    fn default() -> Self {
        Self { show_fps: false, show_fps_override: None,
            responsive_pacing: false, display_sync_pacing: false,
            run_ahead_frames: 0,
            screen_shader: None, screen_bezel: None,
            colour_correction_override: None,
            gb_palettes_override: None, ff_speed_override: None,
            ff_sound_override: None, rewind_override: None,
            turbo_override: None, rumble_override: None,
            eject_save_override: None }
    }
}
impl EmulationProfile {
    pub fn from_settings(settings: &Settings) -> Self {
        Self { show_fps: settings.show_fps,
            show_fps_override: None,
            responsive_pacing: settings.responsive_pacing,
            display_sync_pacing: settings.display_sync_pacing,
            run_ahead_frames: settings.run_ahead_frames,
            screen_shader: None, screen_bezel: None,
            colour_correction_override: None,
            gb_palettes_override: None, ff_speed_override: None,
            ff_sound_override: None, rewind_override: None,
            turbo_override: None, rumble_override: None,
            eject_save_override: None }
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
            (_,25|26|27)=>match platform {
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
            25 | 26 | 27 => self.screen_bezel=Some(view.bezel_for(platform)),
            3 if platform==Platform::Gb =>
                self.gb_palettes_override=Some(view.gb_palettes),
            4 => self.ff_speed_override=Some(view.ff_speed),
            5 => self.ff_sound_override=Some(view.ff_sound),
            6 => self.rewind_override=Some(view.rewind),
            7 => self.turbo_override=Some(view.turbo),
            8 => self.rumble_override=Some(view.rumble),
            9 => self.eject_save_override=Some(view.eject_save),
            _ => return false,
        }
        true
    }

    pub fn sanitize(mut self) -> Self {
        if !self.responsive_pacing { self.display_sync_pacing = false; }
        if self.run_ahead_frames > 2 { self.run_ahead_frames = 0; }
        if !settings::SPEEDS.contains(&self.ff_speed_override.unwrap_or(2)) {
            self.ff_speed_override=None;
        }
        self
    }
    pub fn change(&mut self, key: usize, right: bool) -> bool {
        let before = *self;
        match key {
            10 => {
                self.show_fps = !self.show_fps;
                self.show_fps_override = Some(self.show_fps);
            },
            11 => settings::cycle_pacing(&mut self.responsive_pacing,
                &mut self.display_sync_pacing, right),
            12 => self.run_ahead_frames =
                settings::cycle_u8(self.run_ahead_frames, 2, right),
            _ => return false,
        }
        *self != before
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
        match fs::read(Self::path(root, uri, core)) {
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                // A reset must mask legacy ROM-only overrides for this core.
                if value.get("inherit_global").and_then(|v| v.as_bool()) == Some(true) {
                    return None;
                }
                serde_json::from_value::<Self>(value).ok().map(Self::sanitize)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let bytes = fs::read(Self::legacy_path(root, uri)).ok()?;
                serde_json::from_slice::<Self>(&bytes).ok().map(Self::sanitize)
            }
            Err(_) => None,
        }
    }
    fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
        fs::create_dir_all(path.parent().expect("app-owned profile path"))?;
        let tmp = path.with_extension("json.partial");
        let result = fs::write(&tmp, data).and_then(|_| fs::rename(&tmp, path));
        if result.is_err() { let _ = fs::remove_file(&tmp); }
        result
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
        settings.responsive_pacing = true;
        let mut profile = EmulationProfile::from_settings(&settings);
        assert!(profile.responsive_pacing);
        assert!(profile.change(12, true));
        assert_eq!(profile.run_ahead_frames, 1);
        assert!(profile.change(12, true));
        assert_eq!(profile.run_ahead_frames, 2);
        assert!(profile.change(12, true));
        assert_eq!(profile.run_ahead_frames, 0);
        assert_eq!(EmulationProfile::default().show_fps_override, None);
    }

    #[test]
    fn pre_beta6_profile_inherits_original_responsive_mode() {
        let old: EmulationProfile =
            serde_json::from_str(r#"{"responsive_pacing":true,"run_ahead_frames":1}"#).unwrap();
        assert!(old.responsive_pacing);
        assert!(!old.display_sync_pacing);
        assert_eq!(old.run_ahead_frames, 1);
        let mut changed = old;
        assert!(changed.change(11, true));
        assert!(changed.display_sync_pacing);
        assert!(changed.change(11, true));
        assert!(!changed.responsive_pacing && !changed.display_sync_pacing);
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
        assert_eq!(scoped.bezel_gbc,Bezel::None);
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
