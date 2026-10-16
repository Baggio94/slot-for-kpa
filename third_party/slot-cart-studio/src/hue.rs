//! Where a label's gradient gets its colour.

use crate::{art, cart};

/// HSL to 8-bit sRGB, for the gradient's two stops.
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let sector = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// The hue slot already gives this cart's text-only label: FNV-1a over the cleaned title, modulo
/// 360, the hash `cart::label_colour` uses. A cart with no box art to go on keeps the colour it
/// has on the shelf today.
pub fn fallback_hue(stem: &str) -> u16 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in cart::clean_label(stem).as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h % 360) as u16
}

/// sRGB to HSL, the way out of a colour picked by hand: the studio's stops are stated in HSL, so
/// a chosen deep corner has to be read back into those terms to derive its pale one.
pub fn rgb_to_hsl(rgb: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.0;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s.clamp(0.0, 1.0), l)
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let hue = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (hue, s, max)
}

/// Mean luminance of a logo's opaque pixels: how bright the thing going on the ground actually is,
/// which is what decides whether the ground goes dark under it or pale over it. The near-opaque
/// cut keeps a logo's antialiased rim, which is half ground already, out of the average. 255 for
/// bytes that will not decode, since such a logo is never drawn.
pub fn logo_luma(png: &[u8]) -> f32 {
    let Some((px, w, h)) = art::decode(png) else {
        return 255.0;
    };
    let (mut sum, mut n) = (0.0, 0usize);
    for i in 0..(w * h) as usize {
        if px[i * 4 + 3] < 200 {
            continue;
        }
        sum += 0.2126 * px[i * 4] as f32
            + 0.7152 * px[i * 4 + 1] as f32
            + 0.0722 * px[i * 4 + 2] as f32;
        n += 1;
    }
    if n == 0 {
        255.0
    } else {
        sum / n as f32
    }
}

// A ground used to turn away from a logo of its own hue. The rule is gone, but not because it was
// unnecessary: brightness alone does not settle a clash. `label::stops` puts the ground on the
// opposite side of the logo's brightness, some fifty points of luma apart either way, and the gold
// Pokemon wordmark still reads poorly on the gold its own cover asks for. What the rule could not
// do was land somewhere worth having — a fixed sixty degree turn took Zelda II's blue-violet cover
// into magenta, and tuning the angle only moves which cart it spoils. So the ground follows the
// box art, and a cart whose logo shares that hue is a click on its own colour button.

/// The box art's dominant saturated hue, or `None` when it has none to give: a greyscale cover,
/// or bytes that are not a PNG. The edges are cropped and dull or dark pixels ignored, so a
/// region's trade dress doesn't outvote the art. On slot's own carts this matched the hand-picked
/// hue 4 times in 8, which is why the page lets it be overridden.
pub fn box_hue(png: &[u8]) -> Option<u16> {
    let (px, w, h) = art::decode(png)?;
    let (mx, my) = ((w as f32 * 0.06) as u32, (h as f32 * 0.06) as u32);
    let mut bins = [0f32; 36];
    for y in my..h.saturating_sub(my) {
        for x in mx..w.saturating_sub(mx) {
            let i = ((y * w + x) * 4) as usize;
            if px[i + 3] < 128 {
                continue;
            }
            let (hue, s, v) = rgb_to_hsv(px[i], px[i + 1], px[i + 2]);
            if s < 0.35 || v < 0.25 {
                continue;
            }
            bins[((hue / 10.0) as usize).min(35)] += s * v;
        }
    }
    let (best, weight) =
        bins.iter().enumerate().fold(
            (0, 0.0),
            |acc, (i, w)| if *w > acc.1 { (i, *w) } else { acc },
        );
    (weight > 0.0).then_some(best as u16 * 10 + 5)
}

#[cfg(test)]
#[allow(clippy::manual_range_contains)]
mod tests {
    use super::*;

    /// A logo is mostly clear background, so its tests need the alpha channel `png_rgb` drops.
    fn png_rgba(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let data: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| px(x, y))
            .collect();
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&data).unwrap();
        }
        out
    }

    fn png_rgb(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let data: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| px(x, y))
            .collect();
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&data).unwrap();
        }
        out
    }

    /// `hsv_to_rgb` from slot's cart.rs, verbatim. It is private there, and `fallback_hue` has to
    /// agree with what `label_colour` does with the hue.
    fn slot_hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match (h / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        [
            ((r + m) * 255.0).round() as u8,
            ((g + m) * 255.0).round() as u8,
            ((b + m) * 255.0).round() as u8,
        ]
    }

    #[test]
    fn the_fallback_is_the_colour_slot_already_gives_the_cart() {
        for stem in [
            "Metroid Fusion",
            "Pokemon - Emerald Version (USA, Europe)",
            "Drill Dozer",
            "Apotris",
            "Classic NES Series - Zelda II - The Adventure of Link (USA, Europe)",
        ] {
            let want = cart::label_colour(&cart::clean_label(stem));
            let got = slot_hsv_to_rgb(fallback_hue(stem) as f32, 0.52, 0.74);
            assert_eq!(got, want, "{stem}");
        }
    }

    #[test]
    fn hsl_reaches_the_primaries_and_white() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), [255, 0, 0]);
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), [0, 255, 0]);
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), [0, 0, 255]);
        assert_eq!(hsl_to_rgb(300.0, 0.0, 1.0), [255, 255, 255]);
    }

    #[test]
    fn a_logos_brightness_is_its_opaque_pixels() {
        // White over a clear background: the clear half must not drag the average down.
        let white = png_rgba(20, 20, |x, _| {
            if x < 10 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        assert!(
            (logo_luma(&white) - 255.0).abs() < 0.5,
            "{}",
            logo_luma(&white)
        );

        let black = png_rgba(20, 20, |_, _| [0, 0, 0, 255]);
        assert!(logo_luma(&black) < 0.5, "{}", logo_luma(&black));

        // Bytes that do not decode, and a logo with nothing opaque in it, are never drawn.
        assert_eq!(logo_luma(b"not a png"), 255.0);
        assert_eq!(logo_luma(&png_rgba(8, 8, |_, _| [9, 9, 9, 0])), 255.0);
    }

    #[test]
    fn hsl_survives_the_trip_back_out_of_rgb() {
        for (h, s, l) in [
            (0.0, 0.65, 0.24),
            (120.0, 0.60, 0.46),
            (250.0, 0.55, 0.62),
            (330.0, 0.50, 0.84),
        ] {
            let (h2, s2, l2) = rgb_to_hsl(hsl_to_rgb(h, s, l));
            assert!((h2 - h).abs() <= 1.0, "hue {h} came back {h2}");
            assert!((s2 - s).abs() <= 0.02, "saturation {s} came back {s2}");
            assert!((l2 - l).abs() <= 0.01, "lightness {l} came back {l2}");
        }
    }

    #[test]
    fn a_grey_has_no_hue_and_no_saturation() {
        let (h, s, l) = rgb_to_hsl([128, 128, 128]);
        assert_eq!((h, s), (0.0, 0.0));
        assert!((l - 0.502).abs() < 0.01);
    }

    #[test]
    fn a_red_cover_with_grey_margins_is_red() {
        let png = png_rgb(200, 300, |x, y| {
            if (20..180).contains(&x) && (20..280).contains(&y) {
                [220, 30, 20]
            } else {
                [128, 128, 128]
            }
        });
        assert_eq!(box_hue(&png), Some(5));
    }

    /// Cropping 6% of a 100 px cover removes 6 px from each edge. A saturated blue frame that
    /// thick would outweigh the small green patch if it were counted.
    #[test]
    fn the_cropped_border_does_not_vote() {
        let png = png_rgb(100, 100, |x, y| {
            if x < 6 || x >= 94 || y < 6 || y >= 94 {
                [20, 40, 230]
            } else if (40..60).contains(&x) && (40..60).contains(&y) {
                [40, 200, 40]
            } else {
                [128, 128, 128]
            }
        });
        assert_eq!(box_hue(&png), Some(125));
    }

    #[test]
    fn a_greyscale_cover_has_no_hue() {
        let png = png_rgb(100, 100, |x, y| {
            let g = ((x + y) % 256) as u8;
            [g, g, g]
        });
        assert_eq!(box_hue(&png), None);
    }

    #[test]
    fn bytes_that_are_not_a_png_have_no_hue() {
        assert_eq!(box_hue(b"<html></html>"), None);
    }
}
