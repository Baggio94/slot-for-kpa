use std::path::Path;

use slot_gfx::{ScreenEffect, SRC_H, SRC_W, WHOLE_TEXTURE};
use slot_store::{ini, Platform};

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
        Bezel::Gb | Bezel::Pocket | Bezel::Color => mode != VideoMode::Stretch,
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
    if shader == Shader::Off && platform != Platform::Gba && mode == VideoMode::Aspect {
        ScreenEffect::SharpBilinear
    } else {
        shader.effect()
    }
}

pub fn video_mode_for(root: &Path, stem: &str) -> VideoMode {
    ini::value(root, VIDEO_MODE_FILE, stem)
        .as_deref()
        .and_then(VideoMode::parse)
        .unwrap_or_default()
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
