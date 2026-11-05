use std::path::Path;

use slot_gfx::{ScreenEffect, SRC_H, SRC_W, WHOLE_TEXTURE};
use slot_store::{ini, Platform};

use slot_ui::watomsk::{self, Shape};

use crate::settings::{Bezel, Shader};

pub const VIDEO_MODE_FILE: &str = "Config/video_mode.ini";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VideoMode {
    #[default]
    Actual,
    /// Non-integer: the full panel height at the platform's own aspect ratio.
    Aspect,
    Stretch,
}

impl VideoMode {
    pub fn as_str(self) -> &'static str {
        match self {
            VideoMode::Actual => "actual",
            VideoMode::Aspect => "aspect",
            VideoMode::Stretch => "stretch",
        }
    }

    pub fn parse(s: &str) -> Option<VideoMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "actual" => Some(VideoMode::Actual),
            "aspect" => Some(VideoMode::Aspect),
            "stretch" => Some(VideoMode::Stretch),
            _ => None,
        }
    }

    /// L1: one size up, Actual -> Aspect -> Stretch.
    pub fn bigger(self) -> VideoMode {
        match self {
            VideoMode::Actual => VideoMode::Aspect,
            VideoMode::Aspect | VideoMode::Stretch => VideoMode::Stretch,
        }
    }

    /// R1: one size down, Stretch -> Aspect -> Actual.
    pub fn smaller(self) -> VideoMode {
        match self {
            VideoMode::Stretch => VideoMode::Aspect,
            VideoMode::Aspect | VideoMode::Actual => VideoMode::Actual,
        }
    }
}

/// The official Slot bezel's window is cut for integer scale only, the
/// Watomsk overlays' for the full-height picture (integer fits inside it);
/// nothing can frame a stretched picture.
pub fn bezel_fits(bezel: Bezel, mode: VideoMode) -> bool {
    match bezel {
        Bezel::None => true,
        Bezel::Default => mode == VideoMode::Actual,
        _ => mode != VideoMode::Stretch,
    }
}

/// Why `bezel` is hidden in `mode`, for the short Android message.
pub fn bezel_hidden_message(bezel: Bezel, mode: VideoMode) -> Option<&'static str> {
    if bezel_fits(bezel, mode) {
        return None;
    }
    Some(if mode == VideoMode::Stretch {
        "No bezel in Stretch."
    } else {
        "The slot. bezel needs Integer scale."
    })
}

/// Original Ratio is not a whole multiple of the Game Boy picture: plain
/// pixels would come out 4 or 5 screen pixels wide, so a sprite or a scroll
/// moving one pixel per frame would advance unevenly. With no shader picked,
/// draw it with RetroArch's sharp-bilinear instead, which keeps every pixel
/// the same size and the motion regular.
pub fn screen_effect(shader: Shader, platform: Platform, mode: VideoMode) -> ScreenEffect {
    // A home console's picture is never a whole multiple of the panel in
    // both directions once its aspect is applied: always sharp-bilinear.
    if shader == Shader::Off && !platform.handheld() {
        return ScreenEffect::SharpBilinear;
    }
    if shader == Shader::Off && platform != Platform::Gba && mode == VideoMode::Aspect {
        ScreenEffect::SharpBilinear
    } else {
        shader.effect()
    }
}

pub fn video_mode_for(root: &Path, stem: &str, platform: Platform, panel: (u32, u32)) -> VideoMode {
    ini::value(root, VIDEO_MODE_FILE, stem)
        .as_deref()
        .and_then(VideoMode::parse)
        .unwrap_or_else(|| system_mode(root, platform, panel))
}

/// Settings > Systems' Screen Size: the size a system's games start at
/// until one is given its own with L1/R1 or the pause menu. Kept in the
/// same file as the games', under a key no ROM name starts with.
fn system_key(platform: Platform) -> String {
    format!("*{}", platform.dir_name().to_ascii_lowercase())
}

pub fn system_mode(root: &Path, platform: Platform, panel: (u32, u32)) -> VideoMode {
    ini::value(root, VIDEO_MODE_FILE, &system_key(platform))
        .as_deref()
        .and_then(VideoMode::parse)
        .unwrap_or_else(|| default_mode(platform, panel))
}

pub fn write_system_mode(root: &Path, platform: Platform, mode: VideoMode) -> std::io::Result<()> {
    ini::write(root, VIDEO_MODE_FILE, &system_key(platform), mode.as_str())
}

/// Whether the panel already has this system's shape (within 1%): the
/// picture then fills it and neither Screen Size nor a bezel is offered,
/// as GBA on the KONKR's 3:2 screen.
pub fn screen_fits(platform: Platform, panel: (u32, u32)) -> bool {
    let shape = if platform.handheld() {
        let (w, h) = platform.picture();
        w as f32 / h as f32
    } else {
        4.0 / 3.0
    };
    let panel = panel.0.max(1) as f32 / panel.1.max(1) as f32;
    (panel / shape - 1.0).abs() < 0.01
}

/// Every system starts at Original Ratio, full height (Siham's per-ratio
/// plan, 2026-11-04: GB, GBC and GBA on every screen; GBA already fills a
/// 3:2 panel), except NES and SNES on screens narrower than 4:3: Integer
/// overscale there, in square pixels.
pub fn default_mode(platform: Platform, panel: (u32, u32)) -> VideoMode {
    if square_pixels(platform, panel) { VideoMode::Actual } else { VideoMode::Aspect }
}

fn narrow(panel: (u32, u32)) -> bool {
    panel_aspect(panel) < 4.0 / 3.0 * 0.99
}

/// NES and SNES on screens narrower than 4:3 (the Retroid Pocket Classic,
/// the 1:1 screens) show square pixels, 8:7 for a 256x224 picture: Integer
/// is overscale (the next whole multiple up, a few pixels cut at the
/// edges), then Original Ratio and Stretch (Siham, 2026-11-03).
pub fn square_pixels(platform: Platform, panel: (u32, u32)) -> bool {
    matches!(platform, Platform::Nes | Platform::Snes) && narrow(panel)
}

/// A frame's shape in its console's own pixels: SNES hi-res and interlaced
/// frames (512 wide, 448 tall) count their pixels double.
pub fn square_aspect(frame: (u32, u32)) -> f32 {
    let w = if frame.0 >= 400 { frame.0 / 2 } else { frame.0 };
    let h = if frame.1 >= 400 { frame.1 / 2 } else { frame.1 };
    w.max(1) as f32 / h.max(1) as f32
}

/// How a home console frame fills the screen in `mode`.
pub fn home_fit(platform: Platform, mode: VideoMode, aspect: f32, frame: (u32, u32),
    panel: (u32, u32)) -> slot_gfx::Fit
{
    if square_pixels(platform, panel) {
        let a = square_aspect(frame);
        return match mode {
            VideoMode::Actual => slot_gfx::Fit::Overscale(a),
            VideoMode::Aspect => slot_gfx::Fit::Aspect(a),
            VideoMode::Stretch => slot_gfx::Fit::Stretch,
        };
    }
    match mode {
        VideoMode::Actual => slot_gfx::Fit::Integer(aspect),
        VideoMode::Aspect => slot_gfx::Fit::Aspect(aspect),
        VideoMode::Stretch => slot_gfx::Fit::Stretch,
    }
}

pub fn write_video_mode(root: &Path, stem: &str, mode: VideoMode) -> std::io::Result<()> {
    ini::write(root, VIDEO_MODE_FILE, stem, mode.as_str())
}

/// Where the picture lands on the real panel, in panel pixels: Integer is
/// the largest whole multiple that fits, Original Ratio the largest size at
/// the system's own shape, Stretch the whole panel. On the KONKR's 960x640
/// this is exactly the 3:2 frame's old geometry.
pub fn picture_rect(platform: Platform, mode: VideoMode, panel: (u32, u32)) -> [f32; 4] {
    let (pw, ph) = (panel.0.max(1) as f32, panel.1.max(1) as f32);
    let (w, h) = platform.picture();
    let (w, h) = (w as f32, h as f32);
    let fit = (pw / w).min(ph / h);
    let (dw, dh) = match mode {
        VideoMode::Actual => {
            let k = fit.floor().max(1.0);
            (w * k, h * k)
        }
        VideoMode::Aspect => (w * fit, h * fit),
        VideoMode::Stretch => (pw, ph),
    };
    [((pw - dw) / 2.0).floor(), ((ph - dh) / 2.0).floor(), dw, dh]
}

/// The picture a bezel's window is cut for: the slot. bezel frames the
/// Integer picture, Watomsk's the full Original Ratio one.
pub fn bezel_window(bezel: Bezel, platform: Platform, panel: (u32, u32)) -> [f32; 4] {
    let mode = if bezel == Bezel::Default { VideoMode::Actual } else { VideoMode::Aspect };
    picture_rect(platform, mode, panel)
}

/// A bezel needs room around its window: under 8% of the panel's width on
/// each side (the Retroid Pocket Classic's near-square screen) it would
/// only cover the picture's edge, so it stays hidden.
pub fn bezel_has_room(bezel: Bezel, platform: Platform, panel: (u32, u32)) -> bool {
    let [x, ..] = bezel_window(bezel, platform, panel);
    x >= 0.08 * panel.0 as f32
}

/// How a Watomsk overlay sits on this panel.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WatomskFit {
    /// His side frame drawn for `Shape`, its window over this rect.
    Side(Shape, [f32; 4]),
    /// The picture at the top of the screen (this rect), his logo centred
    /// in the band under it (GBA and Mega Drive on narrower screens).
    Band([f32; 4]),
}

/// Under 4% of the panel on each side (or under it) a frame would only
/// cover the picture's edge: no overlay there.
const ROOM: f32 = 0.04;

fn panel_aspect(panel: (u32, u32)) -> f32 {
    panel.0.max(1) as f32 / panel.1.max(1) as f32
}

/// A home console picture on `panel`, as slot_gfx::fit_window draws it
/// (`top`: at the top of the screen instead of centred).
pub fn home_rect(mode: VideoMode, aspect: f32, frame: (u32, u32), panel: (u32, u32), top: bool)
    -> [f32; 4]
{
    let (ow, oh) = (panel.0.max(1) as f32, panel.1.max(1) as f32);
    let (tw, th) = (frame.0.max(1) as f32, frame.1.max(1) as f32);
    let a = if aspect > 0.0 { aspect } else { tw / th };
    let (pw, ph) = match mode {
        VideoMode::Aspect => if oh * a <= ow { (oh * a, oh) } else { (ow, ow / a) },
        VideoMode::Actual => {
            let mut k = (oh / th).floor().max(1.0);
            while k > 1.0 && th * k * a > ow { k -= 1.0; }
            let ph = th * k;
            ((ph * a).min(ow), ph)
        }
        VideoMode::Stretch => (ow, oh),
    };
    let y = if top { 0.0 } else { (oh - ph) / 2.0 };
    [(ow - pw) / 2.0, y, pw, ph]
}

/// Where `bezel`'s Watomsk overlay goes (Siham's plan, 2026-11-03):
/// GB/GBC beside the Original Ratio picture (not on near-square screens);
/// GBA nowhere on 3:2, beside it on wider screens in Original Ratio
/// (Perfect_GBA's layout), at the bottom otherwise; NES/SNES/Mega Drive
/// beside the 4:3 picture on wider screens, nothing on 4:3, and on
/// narrower screens only the Mega Drive, at the bottom. Never in Stretch.
/// `home` is the console's display aspect and frame size.
pub fn watomsk_fit(bezel: Bezel, platform: Platform, mode: VideoMode, panel: (u32, u32),
    home: (f32, (u32, u32))) -> Option<WatomskFit>
{
    let design = bezel.watomsk()?;
    if !bezel_fits(bezel, mode) || !Bezel::list(platform).contains(&bezel) { return None; }
    let a = panel_aspect(panel);
    let (pw, ph) = (panel.0 as f32, panel.1 as f32);
    let side = |window: [f32; 4]| (window[0] >= ROOM * pw)
        .then(|| WatomskFit::Side(watomsk::nearest_shape(design, panel), window));
    let band = |mut pic: [f32; 4]| {
        pic[1] = 0.0;
        (ph - pic[3] >= ROOM * ph).then_some(WatomskFit::Band(pic))
    };
    match platform {
        Platform::Gb | Platform::Gbc => side(picture_rect(platform, VideoMode::Aspect, panel)),
        Platform::Gba => {
            if screen_fits(platform, panel) { return None; }
            if a > 1.5 && mode == VideoMode::Aspect {
                side(picture_rect(platform, VideoMode::Aspect, panel))
            } else {
                band(picture_rect(platform, mode, panel))
            }
        }
        Platform::Nes | Platform::Snes | Platform::Md => {
            if a > 4.0 / 3.0 * 1.01 {
                side(home_rect(VideoMode::Aspect, 4.0 / 3.0, home.1, panel, false))
            } else if a < 4.0 / 3.0 * 0.99 && watomsk::has_band(design) {
                band(home_rect(mode, home.0, home.1, panel, true))
            } else {
                None
            }
        }
    }
}

/// Whether a system's Bezel row means anything on this panel.
pub fn bezel_offered(platform: Platform, panel: (u32, u32)) -> bool {
    let a = panel_aspect(panel);
    match platform {
        Platform::Gb | Platform::Gbc =>
            picture_rect(platform, VideoMode::Aspect, panel)[0] >= ROOM * panel.0 as f32,
        Platform::Gba | Platform::Md => !screen_fits(platform, panel),
        Platform::Nes | Platform::Snes => a > 4.0 / 3.0 * 1.01,
    }
}

/// The game texture's own picture: GB/GBC sit centred in the 240x160 frame.
pub fn picture_source(platform: Platform) -> [f32; 4] {
    let (w, h) = platform.picture();
    let x = SRC_W.saturating_sub(w) / 2;
    let y = SRC_H.saturating_sub(h) / 2;
    [
        x as f32 / SRC_W as f32,
        y as f32 / SRC_H as f32,
        w as f32 / SRC_W as f32,
        h as f32 / SRC_H as f32,
    ]
}

/// Whether the picture is a whole multiple of the system's pixels.
pub fn whole_scale(platform: Platform, rect: [f32; 4]) -> bool {
    let (w, h) = platform.picture();
    let (kx, ky) = (rect[2] / w as f32, rect[3] / h as f32);
    kx >= 1.0 && (kx - kx.round()).abs() < 0.01 && (ky - ky.round()).abs() < 0.01
}

pub fn source_rect(platform: Platform, mode: VideoMode) -> [f32; 4] {
    let (w, h) = platform.picture();
    if mode == VideoMode::Actual || (w, h) == (SRC_W, SRC_H) {
        return WHOLE_TEXTURE;
    }
    let y = SRC_H.saturating_sub(h) / 2;
    if mode == VideoMode::Aspect {
        // Widen the crop to the panel's shape so the picture keeps its ratio.
        let shown = SRC_W as f32 * h as f32 / SRC_H as f32;
        let x = (SRC_W as f32 - shown) / 2.0;
        return [
            x / SRC_W as f32,
            y as f32 / SRC_H as f32,
            shown / SRC_W as f32,
            h as f32 / SRC_H as f32,
        ];
    }
    let x = SRC_W.saturating_sub(w) / 2;
    [
        x as f32 / SRC_W as f32,
        y as f32 / SRC_H as f32,
        w as f32 / SRC_W as f32,
        h as f32 / SRC_H as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_konkr_keeps_its_three_sizes() {
        let kpa = (960, 640);
        assert_eq!(picture_rect(Platform::Gb, VideoMode::Actual, kpa), [160.0, 32.0, 640.0, 576.0]);
        let [x, y, w, h] = picture_rect(Platform::Gb, VideoMode::Aspect, kpa);
        assert_eq!((y, h), (0.0, 640.0));
        assert!((w - 711.111).abs() < 0.01 && x == 124.0);
        assert_eq!(picture_rect(Platform::Gb, VideoMode::Stretch, kpa), [0.0, 0.0, 960.0, 640.0]);
        for mode in [VideoMode::Actual, VideoMode::Aspect, VideoMode::Stretch] {
            assert_eq!(picture_rect(Platform::Gba, mode, kpa), [0.0, 0.0, 960.0, 640.0]);
        }
        for bezel in [Bezel::Default, Bezel::Gb, Bezel::Color] {
            assert!(bezel_has_room(bezel, Platform::Gb, kpa));
        }
    }

    #[test]
    fn other_panels_use_their_whole_screen() {
        let rpc = (1240, 1080);
        assert_eq!(picture_rect(Platform::Gb, VideoMode::Actual, rpc), [60.0, 36.0, 1120.0, 1008.0]);
        assert_eq!(picture_rect(Platform::Gba, VideoMode::Actual, rpc), [20.0, 140.0, 1200.0, 800.0]);
        assert!(!bezel_has_room(Bezel::Default, Platform::Gb, rpc));
        assert!(!bezel_has_room(Bezel::Gb, Platform::Gbc, rpc));
        let wide = (1920, 1080);
        assert_eq!(picture_rect(Platform::Gba, VideoMode::Actual, wide), [240.0, 60.0, 1440.0, 960.0]);
        assert!(bezel_has_room(Bezel::Gb, Platform::Gb, wide));
        let four_three = (1440, 1080);
        assert!(bezel_has_room(Bezel::Pocket, Platform::Gb, four_three));
        assert!(whole_scale(Platform::Gb, picture_rect(Platform::Gb, VideoMode::Actual, four_three)));
        assert!(!whole_scale(Platform::Gb, picture_rect(Platform::Gb, VideoMode::Aspect, four_three)));
    }

    #[test]
    fn watomsk_overlays_follow_the_plan_on_every_screen() {
        use WatomskFit::*;
        use VideoMode::*;
        let nes = (4.0 / 3.0, (256, 240));
        let gb = |b, p, m, panel| watomsk_fit(b, p, m, panel, nes);
        // GB/GBC: beside the picture on 3:2, 4:3, 16:9; not on squares.
        assert!(matches!(gb(Bezel::Gb, Platform::Gb, Aspect, (960, 640)), Some(Side(Shape::ThreeTwo, _))));
        assert!(matches!(gb(Bezel::Pocket, Platform::Gb, Actual, (1024, 768)), Some(Side(Shape::FourThree, _))));
        assert!(matches!(gb(Bezel::Color, Platform::Gbc, Aspect, (1920, 1080)), Some(Side(Shape::SixteenNine, _))));
        assert_eq!(gb(Bezel::Gb, Platform::Gb, Aspect, (720, 720)), None);
        assert_eq!(gb(Bezel::Gb, Platform::Gb, Aspect, (1240, 1080)), None);
        assert_eq!(gb(Bezel::Gb, Platform::Gb, Stretch, (960, 640)), None);
        assert_eq!(gb(Bezel::Color, Platform::Gb, Aspect, (960, 640)), None, "not a GB frame");
        // GBA: nothing on 3:2, Perfect_GBA's side frame on 16:9 Original,
        // the band under the picture otherwise.
        let gba = Bezel::GbaClassic;
        assert_eq!(gb(gba, Platform::Gba, Actual, (960, 640)), None);
        assert!(matches!(gb(gba, Platform::Gba, Aspect, (1920, 1080)), Some(Side(Shape::SixteenNine, _))));
        assert_eq!(gb(gba, Platform::Gba, Actual, (1920, 1080)), Some(Band([240.0, 0.0, 1440.0, 960.0])));
        assert_eq!(gb(gba, Platform::Gba, Aspect, (1024, 768)), Some(Band([0.0, 0.0, 1024.0, 682.6667])));
        assert_eq!(gb(gba, Platform::Gba, Actual, (1240, 1080)), Some(Band([20.0, 0.0, 1200.0, 800.0])));
        assert_eq!(gb(gba, Platform::Gba, Actual, (720, 720)), Some(Band([0.0, 0.0, 720.0, 480.0])));
        // NES/SNES: beside on 3:2 and 16:9, nothing on 4:3 or squares.
        assert!(matches!(gb(Bezel::Nes, Platform::Nes, Aspect, (960, 640)),
            Some(Side(Shape::ThreeTwo, [x, 0.0, _, 640.0])) if (x - 53.33).abs() < 0.1));
        assert!(matches!(gb(Bezel::SnesAlt, Platform::Snes, Actual, (1920, 1080)), Some(Side(Shape::SixteenNine, _))));
        assert_eq!(gb(Bezel::Nes, Platform::Nes, Aspect, (1024, 768)), None);
        assert_eq!(gb(Bezel::Snes, Platform::Snes, Aspect, (1240, 1080)), None);
        // Mega Drive: and at the bottom of squares.
        let md = (4.0 / 3.0, (320, 224));
        assert_eq!(watomsk_fit(Bezel::GenesisClassic, Platform::Md, Aspect, (720, 720), md),
            Some(Band([0.0, 0.0, 720.0, 540.0])));
        assert_eq!(watomsk_fit(Bezel::MegaDrive, Platform::Md, Aspect, (1024, 768), md), None);
        assert!(bezel_offered(Platform::Md, (720, 720)) && !bezel_offered(Platform::Snes, (720, 720)));
        assert!(!bezel_offered(Platform::Gba, (960, 640)) && bezel_offered(Platform::Nes, (960, 640)));
        assert!(!bezel_offered(Platform::Gb, (1240, 1080)));
    }

    #[test]
    fn game_boys_start_at_original_ratio_on_every_screen() {
        for panel in [(1240, 1080), (960, 640), (1440, 1080), (1920, 1080), (720, 720)] {
            for platform in [Platform::Gb, Platform::Gbc, Platform::Gba] {
                assert_eq!(default_mode(platform, panel), VideoMode::Aspect, "{platform:?} {panel:?}");
            }
        }
    }

    #[test]
    fn nes_and_snes_use_square_pixels_on_narrow_screens() {
        use slot_gfx::Fit;
        let rpc = (1240, 1080);
        assert_eq!(default_mode(Platform::Snes, rpc), VideoMode::Actual);
        assert_eq!(default_mode(Platform::Nes, (720, 720)), VideoMode::Actual);
        assert_eq!(default_mode(Platform::Md, rpc), VideoMode::Aspect);
        assert_eq!(default_mode(Platform::Snes, (960, 640)), VideoMode::Aspect);
        assert_eq!(home_fit(Platform::Snes, VideoMode::Actual, 4.0 / 3.0, (512, 448), rpc),
            Fit::Overscale(256.0 / 224.0));
        assert_eq!(home_fit(Platform::Nes, VideoMode::Aspect, 4.0 / 3.0, (256, 240), rpc),
            Fit::Aspect(256.0 / 240.0));
        assert_eq!(home_fit(Platform::Nes, VideoMode::Actual, 4.0 / 3.0, (256, 240), (960, 640)),
            Fit::Integer(4.0 / 3.0));
    }

    #[test]
    fn shoulders_step_through_the_three_sizes() {
        assert_eq!(VideoMode::Actual.bigger(), VideoMode::Aspect);
        assert_eq!(VideoMode::Aspect.bigger(), VideoMode::Stretch);
        assert_eq!(VideoMode::Stretch.bigger(), VideoMode::Stretch);
        assert_eq!(VideoMode::Stretch.smaller(), VideoMode::Aspect);
        assert_eq!(VideoMode::Aspect.smaller(), VideoMode::Actual);
        assert_eq!(VideoMode::Actual.smaller(), VideoMode::Actual);
        assert_eq!(VideoMode::parse("aspect"), Some(VideoMode::Aspect));
    }

    #[test]
    fn aspect_keeps_the_game_boy_ratio_on_a_3_2_panel() {
        let [x, y, w, h] = source_rect(Platform::Gb, VideoMode::Aspect);
        let (tw, th) = (w * SRC_W as f32, h * SRC_H as f32);
        assert!((th - 144.0).abs() < 1e-3);
        assert!((tw / th - SRC_W as f32 / SRC_H as f32).abs() < 1e-4);
        assert!((x + w / 2.0 - 0.5).abs() < 1e-6 && (y + h / 2.0 - 0.5).abs() < 1e-6);
        // Shown on the whole panel, the 160 game columns keep 10:9.
        let game_w = 160.0 / tw * 3.0;
        assert!((game_w / 2.0 - 10.0 / 9.0).abs() < 1e-3);
    }

    #[test]
    fn original_ratio_without_shader_keeps_pixels_even() {
        assert_eq!(screen_effect(Shader::Off, Platform::Gb, VideoMode::Aspect),
            ScreenEffect::SharpBilinear);
        assert_eq!(screen_effect(Shader::Off, Platform::Gbc, VideoMode::Aspect),
            ScreenEffect::SharpBilinear);
        assert_eq!(screen_effect(Shader::Off, Platform::Gb, VideoMode::Actual),
            ScreenEffect::None);
        assert_eq!(screen_effect(Shader::Grid, Platform::Gb, VideoMode::Aspect),
            ScreenEffect::Grid);
        assert_eq!(screen_effect(Shader::Off, Platform::Gba, VideoMode::Aspect),
            ScreenEffect::None);
    }

    #[test]
    fn each_bezel_fits_only_its_sizes() {
        use VideoMode::*;
        for mode in [Actual, Aspect, Stretch] {
            assert!(bezel_fits(Bezel::None, mode));
        }
        assert!(bezel_fits(Bezel::Default, Actual));
        assert!(!bezel_fits(Bezel::Default, Aspect));
        for watomsk in [Bezel::Gb, Bezel::Pocket, Bezel::Color] {
            assert!(bezel_fits(watomsk, Actual));
            assert!(bezel_fits(watomsk, Aspect));
            assert!(!bezel_fits(watomsk, Stretch));
            assert_eq!(bezel_hidden_message(watomsk, Stretch), Some("No bezel in Stretch."));
        }
        assert_eq!(bezel_hidden_message(Bezel::Default, Stretch), Some("No bezel in Stretch."));
        assert!(bezel_hidden_message(Bezel::Default, Aspect).unwrap().contains("Integer"));
        assert_eq!(bezel_hidden_message(Bezel::Gb, Aspect), None);
    }
}

#[cfg(test)]
mod watomsk_preview {
    use super::*;

    /// SLOT_OVERLAY_PREVIEW=<dir> cargo test -p slot-android watomsk_preview -- --ignored
    #[test]
    #[ignore]
    fn write_watomsk_previews() {
        use slot_ui::fit_overlay::{fit_band, fit_overlay};
        let Ok(dir) = std::env::var("SLOT_OVERLAY_PREVIEW") else { return };
        let panels = [(960, 640), (1024, 768), (1920, 1080), (720, 720), (1240, 1080), (1280, 800)];
        let cases = [(Platform::Gb, Bezel::Gb), (Platform::Gbc, Bezel::Color),
            (Platform::Gba, Bezel::GbaClassic), (Platform::Nes, Bezel::Nes),
            (Platform::Snes, Bezel::SnesAlt), (Platform::Md, Bezel::GenesisClassic),
            (Platform::Md, Bezel::MegaDrive)];
        for panel in panels {
            for (platform, bezel) in cases {
                for mode in [VideoMode::Actual, VideoMode::Aspect] {
                    let home = (4.0 / 3.0, if platform == Platform::Md { (320, 224) } else { (256, 224) });
                    let Some(fit) = watomsk_fit(bezel, platform, mode, panel, home) else { continue };
                    let design = bezel.watomsk().unwrap();
                    let (rgba, pic) = match fit {
                        WatomskFit::Side(shape, window) => {
                            let side = slot_ui::watomsk::side(design, shape).unwrap();
                            (fit_overlay(&side.art(), panel, window), window)
                        }
                        WatomskFit::Band(pic) => {
                            let band = slot_ui::watomsk::band(design).unwrap();
                            (fit_band(&band.art(), panel, pic), pic)
                        }
                    };
                    let name = format!("{dir}/{:?}-{:?}-{}-{}x{}-pic{}_{}_{}_{}.rgba", platform, bezel,
                        mode.as_str(), panel.0, panel.1,
                        pic[0].round(), pic[1].round(), pic[2].round(), pic[3].round());
                    std::fs::write(name, rgba).unwrap();
                }
            }
        }
    }
}
