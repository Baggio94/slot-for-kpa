//! Fit a screen bezel drawn for the KONKR's 3:2 panel to any panel.
//!
//! A bezel is a frame around a window cut for the picture, with side panels
//! carrying its art (LED, logo). The window column is scaled so the window
//! lands exactly on the picture; each side panel's art is centred in the
//! room left on its side (shrunk only when that room is narrower), over a
//! mirrored fill of the panel's plain texture. On the KONKR the result is
//! the original bezel, pixel for pixel in layout.

/// One bezel picture and where its parts are, in its own pixels.
pub struct OverlayArt<'a> {
    pub rgba: &'a [u8],
    pub size: (u32, u32),
    /// The picture the window is cut for: x0, y0, x1, y1.
    pub window: [f32; 4],
    /// The window's column, frame included: x0, x1.
    pub column: [f32; 2],
    /// Plain texture on the left and right panels, to fill wider sides.
    pub plain: [[f32; 2]; 2],
}

/// The slot. GB/GBC bezel (720x480): window cut for 3x integer.
pub fn slot_bezel(rgba: &[u8]) -> OverlayArt<'_> {
    OverlayArt {
        rgba, size: (720, 480),
        window: [120.0, 24.0, 600.0, 456.0],
        column: [111.0, 609.0],
        plain: [[97.0, 109.0], [612.0, 716.0]],
    }
}

/// Watomsk's GB/Pocket/Color overlays (960x640): window cut for the full
/// height at 10:9.
pub fn watomsk(rgba: &[u8]) -> OverlayArt<'_> {
    let x0 = (960.0 - 640.0 * 10.0 / 9.0) / 2.0;
    OverlayArt {
        rgba, size: (960, 640),
        window: [x0, 0.0, 960.0 - x0, 640.0],
        column: [x0, 960.0 - x0],
        plain: [[2.0, 12.0], [948.0, 958.0]],
    }
}

/// Render `art` for a panel of `out` pixels whose picture sits at `pic`
/// (x, y, w, h in panel pixels). Returns RGBA, `out.0 * out.1 * 4` bytes.
pub fn fit_overlay(art: &OverlayArt, out: (u32, u32), pic: [f32; 4]) -> Vec<u8> {
    let (ow, oh) = out;
    let [wx0, wy0, wx1, _] = art.window;
    let k = pic[3] / (art.window[3] - wy0);
    // Source y for an output row, aligned with the window.
    let src_y = |y: f32| wy0 + (y - pic[1]) / k;
    let col_left = pic[0] - (wx0 - art.column[0]) * k;
    let col_right = pic[0] + pic[2] + (art.column[1] - wx1) * k;
    let (sw, sh) = (art.size.0 as f32, art.size.1 as f32);
    // Each side's art: its panel strip, scaled to fit the room it has.
    let sides = [
        (0.0, col_left, 0.0, art.column[0], art.plain[0]),
        (col_right, ow as f32, art.column[1], sw, art.plain[1]),
    ].map(|(from, to, s0, s1, plain)| {
        let room = (to - from).max(0.0);
        let ks = k.min(room / (s1 - s0));
        let x = from + (room - (s1 - s0) * ks) / 2.0;
        // Vertically the strip stays centred on the picture.
        let y = pic[1] + pic[3] / 2.0 - (wy0 + art.window[3]) / 2.0 * ks;
        (from, to, s0, s1, ks, x, y, plain)
    });
    let mut rgba = vec![0u8; (ow * oh * 4) as usize];
    for oy in 0..oh {
        let y = oy as f32 + 0.5;
        for ox in 0..ow {
            let x = ox as f32 + 0.5;
            let (fx, fy) = if x >= col_left && x < col_right {
                (wx0 + (x - pic[0]) / k, src_y(y))
            } else {
                let side = if x < col_left { &sides[0] } else { &sides[1] };
                let (from, _, s0, s1, ks, ax, ay, plain) = *side;
                let (u, v) = ((x - ax) / ks + s0, (y - ay) / ks);
                if u >= s0 && u < s1 && v >= 0.0 && v < sh {
                    (u, v)
                } else {
                    // Mirrored plain texture, continuous from the column.
                    let span = plain[1] - plain[0];
                    let t = ((x - from) / k).rem_euclid(2.0 * span);
                    let u = plain[0] + if t < span { t } else { 2.0 * span - t };
                    (u, src_y(y))
                }
            };
            let px = sample(art, fx, mirror(fy, sh));
            rgba[((oy * ow + ox) * 4) as usize..][..4].copy_from_slice(&px);
        }
    }
    rgba
}

fn mirror(v: f32, len: f32) -> f32 {
    let t = v.rem_euclid(2.0 * len);
    if t < len { t } else { 2.0 * len - t - 0.001 }
}

/// Bilinear sample at source pixel coordinates (pixel centres at +0.5).
fn sample(art: &OverlayArt, x: f32, y: f32) -> [u8; 4] {
    let (w, h) = (art.size.0 as i32, art.size.1 as i32);
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |xi: i32, yi: i32| {
        let (xi, yi) = (xi.clamp(0, w - 1), yi.clamp(0, h - 1));
        let i = ((yi * w + xi) * 4) as usize;
        let p = &art.rgba[i..i + 4];
        // Premultiplied, so transparent texels never darken the edge.
        let a = p[3] as f32 / 255.0;
        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, p[3] as f32]
    };
    let (x0, y0) = (x0 as i32, y0 as i32);
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    let mut out = [0u8; 4];
    let alpha = (a[3] * (1.0 - tx) + b[3] * tx) * (1.0 - ty) + (c[3] * (1.0 - tx) + d[3] * tx) * ty;
    for ch in 0..3 {
        let v = (a[ch] * (1.0 - tx) + b[ch] * tx) * (1.0 - ty)
            + (c[ch] * (1.0 - tx) + d[ch] * tx) * ty;
        out[ch] = if alpha > 0.0 { (v * 255.0 / alpha).round().clamp(0.0, 255.0) as u8 } else { 0 };
    }
    out[3] = alpha.round().clamp(0.0, 255.0) as u8;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(rgba: &[u8]) -> OverlayArt<'_> {
        OverlayArt {
            rgba, size: (12, 4),
            window: [4.0, 0.0, 8.0, 4.0],
            column: [3.0, 9.0],
            plain: [[0.0, 1.0], [11.0, 12.0]],
        }
    }

    #[test]
    fn at_its_own_size_the_bezel_is_unchanged() {
        let rgba: Vec<u8> = (0..12 * 4).flat_map(|i| [i as u8 * 5, 7, 9, 255]).collect();
        let out = fit_overlay(&art(&rgba), (12, 4), [4.0, 0.0, 4.0, 4.0]);
        assert_eq!(out, rgba);
    }

    #[test]
    fn a_wider_panel_keeps_the_window_on_the_picture() {
        let mut rgba = vec![255u8; 12 * 4 * 4];
        for y in 0..4 {
            for x in 4..8 {
                rgba[(y * 12 + x) * 4 + 3] = 0;
            }
        }
        let out = fit_overlay(&art(&rgba), (24, 8), [8.0, 0.0, 8.0, 8.0]);
        for y in 0..8 {
            for x in (0..7).chain(9..15).chain(17..24) {
                let inside = (8..16).contains(&x);
                assert_eq!(out[(y * 24 + x) * 4 + 3] == 0, inside, "{x},{y}");
            }
        }
    }
}

#[cfg(test)]
mod preview {
    use super::*;

    /// SLOT_OVERLAY_PREVIEW=<dir> cargo test -p slot-ui preview -- --ignored
    #[test]
    #[ignore]
    fn write_previews() {
        let Ok(dir) = std::env::var("SLOT_OVERLAY_PREVIEW") else { return };
        let panels = [("kpa", 960, 640), ("rpc", 1240, 1080), ("4x3", 1440, 1080),
            ("16x9", 1920, 1080), ("16x10", 1920, 1200)];
        for (name, w, h) in panels {
            let gb = crate::bitmaps::bezel_gb();
            let fit = (w as f32 / 160.0).min(h as f32 / 144.0);
            let k = fit.floor();
            let pic = [((w as f32 - 160.0 * k) / 2.0).floor(), ((h as f32 - 144.0 * k) / 2.0).floor(),
                160.0 * k, 144.0 * k];
            let out = fit_overlay(&slot_bezel(gb), (w, h), pic);
            std::fs::write(format!("{dir}/slot-{name}-{w}x{h}.rgba"), out).unwrap();
            let pic = [((w as f32 - 160.0 * fit) / 2.0).floor(), ((h as f32 - 144.0 * fit) / 2.0).floor(),
                160.0 * fit, 144.0 * fit];
            let out = fit_overlay(&watomsk(crate::bitmaps::overlay_color()), (w, h), pic);
            std::fs::write(format!("{dir}/watomsk-{name}-{w}x{h}.rgba"), out).unwrap();
        }
    }
}
