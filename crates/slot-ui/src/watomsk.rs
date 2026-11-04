//! Watomsk's overlays (github.com/Watomsk/Overlays, used with his
//! permission), recomposed for each screen shape by
//! tools/watomsk-overlays/export.py. Black only (his dark grey versions
//! were dropped, Siham 2026-11-04).

use crate::fit_overlay::{BandArt, OverlayArt};

/// One of his designs. The Game Boy ones are drawn beside the picture;
/// GBA and Mega Drive also have a logo for the band under the picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Design {
    Gb, Pocket, Color,
    GbaClassic, Gba, GbaSp, GbaAlt,
    Nes, NesGrayLogo,
    Snes, SnesAlt, SnesGrayLogo, SnesGrayLogoAlt,
    GenesisClassic, Genesis, MegaDrive,
}

/// The screen shape a side frame was drawn for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Shape { ThreeTwo, FourThree, SixteenNine }

impl Shape {
    pub fn aspect(self) -> f32 {
        match self {
            Shape::ThreeTwo => 1.5,
            Shape::FourThree => 4.0 / 3.0,
            Shape::SixteenNine => 16.0 / 9.0,
        }
    }
}

macro_rules! files {
    ($($name:literal),* $(,)?) => {
        fn file(name: &str) -> Option<&'static [u8]> {
            match name {
                $($name => Some(include_bytes!(
                    concat!("../../../assets/watomsk/", $name, ".png")).as_slice()),)*
                _ => None,
            }
        }
    };
}

files!(
        "gb-16x9",
        "gb-3x2",
        "gb-4x3",
        "gba-16x9",
        "gba-alt-16x9",
        "gba-alt-band",
        "gba-band",
        "gba-classic-16x9",
        "gba-classic-band",
        "gbasp-16x9",
        "gbasp-band",
        "gbc-16x9",
        "gbc-3x2",
        "gbc-4x3",
        "gbp-16x9",
        "gbp-3x2",
        "gbp-4x3",
        "genesis-16x9",
        "genesis-3x2",
        "genesis-band",
        "genesis-classic-16x9",
        "genesis-classic-3x2",
        "genesis-classic-band",
        "megadrive-16x9",
        "megadrive-3x2",
        "megadrive-band",
        "nes-16x9",
        "nes-3x2",
        "nes-graylogo-16x9",
        "nes-graylogo-3x2",
        "snes-16x9",
        "snes-3x2",
        "snes-alt-16x9",
        "snes-alt-3x2",
        "snes-graylogo-16x9",
        "snes-graylogo-3x2",
        "snes-graylogo-alt-16x9",
        "snes-graylogo-alt-3x2",
);

fn key(design: Design) -> &'static str {
    use Design::*;
    match design {
        Gb => "gb", Pocket => "gbp", Color => "gbc",
        GbaClassic => "gba-classic", Gba => "gba", GbaSp => "gbasp", GbaAlt => "gba-alt",
        Nes => "nes", NesGrayLogo => "nes-graylogo",
        Snes => "snes", SnesAlt => "snes-alt", SnesGrayLogo => "snes-graylogo",
        SnesGrayLogoAlt => "snes-graylogo-alt",
        GenesisClassic => "genesis-classic", Genesis => "genesis", MegaDrive => "megadrive",
    }
}

/// The files are <key>-<part>.png.
fn bytes(design: Design, part: &str) -> Option<&'static [u8]> {
    file(&format!("{}-{}", key(design), part))
}

fn part(shape: Shape) -> &'static str {
    match shape {
        Shape::ThreeTwo => "3x2",
        Shape::FourThree => "4x3",
        Shape::SixteenNine => "16x9",
    }
}

/// The shapes this design has a side frame for.
pub fn shapes(design: Design) -> &'static [Shape] {
    use Design::*;
    match design {
        Gb | Pocket | Color => &[Shape::ThreeTwo, Shape::FourThree, Shape::SixteenNine],
        GbaClassic | Gba | GbaSp | GbaAlt => &[Shape::SixteenNine],
        _ => &[Shape::ThreeTwo, Shape::SixteenNine],
    }
}

/// The side frame drawn for the shape nearest the panel's.
pub fn nearest_shape(design: Design, panel: (u32, u32)) -> Shape {
    let a = panel.0.max(1) as f32 / panel.1.max(1) as f32;
    *shapes(design).iter()
        .min_by(|x, y| (x.aspect() / a).ln().abs().total_cmp(&(y.aspect() / a).ln().abs()))
        .unwrap_or(&Shape::SixteenNine)
}

/// The window's shape in the side frames: the system's own picture.
fn window_aspect(design: Design) -> f32 {
    use Design::*;
    match design {
        Gb | Pocket | Color => 10.0 / 9.0,
        GbaClassic | Gba | GbaSp | GbaAlt => 1.5,
        _ => 4.0 / 3.0,
    }
}

/// A decoded side frame and where its window is.
pub struct Side {
    pub rgba: Vec<u8>,
    pub size: (u32, u32),
    window: [f32; 4],
}

impl Side {
    pub fn art(&self) -> OverlayArt<'_> {
        let (w, _) = self.size;
        let w = w as f32;
        OverlayArt {
            rgba: &self.rgba,
            size: self.size,
            window: self.window,
            column: [self.window[0], self.window[2]],
            plain: [[2.0, 12.0], [w - 12.0, w - 2.0]],
        }
    }
}

pub fn side(design: Design, shape: Shape) -> Option<Side> {
    let bytes = bytes(design, part(shape))?;
    let (rgba, w, h) = crate::art::decode_png_bytes(bytes)?;
    let ww = (h as f32 * window_aspect(design)).round();
    let x0 = ((w as f32 - ww) / 2.0).floor();
    Some(Side { rgba, size: (w, h), window: [x0, 0.0, x0 + ww, h as f32] })
}

/// A decoded band logo.
pub struct Band {
    pub rgba: Vec<u8>,
    pub size: (u32, u32),
}

impl Band {
    pub fn art(&self) -> BandArt<'_> {
        BandArt { rgba: &self.rgba, size: self.size }
    }
}

pub fn band(design: Design) -> Option<Band> {
    let bytes = bytes(design, "band")?;
    let (rgba, w, h) = crate::art::decode_png_bytes(bytes)?;
    Some(Band { rgba, size: (w, h) })
}

pub fn has_band(design: Design) -> bool {
    bytes(design, "band").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Design; 16] = [
        Design::Gb, Design::Pocket, Design::Color,
        Design::GbaClassic, Design::Gba, Design::GbaSp, Design::GbaAlt,
        Design::Nes, Design::NesGrayLogo,
        Design::Snes, Design::SnesAlt, Design::SnesGrayLogo, Design::SnesGrayLogoAlt,
        Design::GenesisClassic, Design::Genesis, Design::MegaDrive,
    ];

    #[test]
    fn every_design_has_its_frames() {
        for design in ALL {
            {
                for &shape in shapes(design) {
                    let side = side(design, shape).expect("side frame decodes");
                    let [x0, y0, x1, y1] = side.window;
                    let (w, h) = side.size;
                    // The window is see-through in its middle.
                    let (cx, cy) = (((x0 + x1) / 2.0) as u32, ((y0 + y1) / 2.0) as u32);
                    assert_eq!(side.rgba[((cy * w + cx) * 4 + 3) as usize], 0, "{design:?}");
                    assert!((shape.aspect() - w as f32 / h as f32).abs() < 0.01, "{design:?}");
                }
                let wants_band = matches!(design, Design::GbaClassic | Design::Gba | Design::GbaSp
                    | Design::GbaAlt | Design::GenesisClassic | Design::Genesis | Design::MegaDrive);
                assert_eq!(band(design).is_some(), wants_band, "{design:?}");
            }
        }
    }

    #[test]
    fn the_nearest_shape_follows_the_panel() {
        assert_eq!(nearest_shape(Design::Gb, (960, 640)), Shape::ThreeTwo);
        assert_eq!(nearest_shape(Design::Gb, (1024, 768)), Shape::FourThree);
        assert_eq!(nearest_shape(Design::Gb, (1920, 1080)), Shape::SixteenNine);
        assert_eq!(nearest_shape(Design::Nes, (1280, 800)), Shape::ThreeTwo);
        assert_eq!(nearest_shape(Design::Snes, (2400, 1080)), Shape::SixteenNine);
        assert_eq!(nearest_shape(Design::Gba, (1920, 1080)), Shape::SixteenNine);
    }
}
