use slot_retro::{GBA_H, GBA_W};

pub fn png(xrgb8888: &[u8]) -> Option<Vec<u8>> {
    let n = (GBA_W * GBA_H) as usize;
    if xrgb8888.len() < n * 4 {
        return None;
    }
    let mut rgb = Vec::with_capacity(n * 3);
    for px in xrgb8888[..n * 4].chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    encode(&rgb)
}

/// A home console frame of any size as the same 240x160 thumbnail: the
/// picture keeps its display aspect, black bars fill the rest.
pub fn png_fit(xrgb8888: &[u8], (w, h): (u32, u32), aspect: f32) -> Option<Vec<u8>> {
    if (w, h) == (GBA_W, GBA_H) && aspect <= 0.0 {
        return png(xrgb8888);
    }
    if w == 0 || h == 0 || xrgb8888.len() < (w * h * 4) as usize {
        return None;
    }
    let aspect = if aspect > 0.0 { aspect } else { w as f32 / h as f32 };
    let (tw, th) = (GBA_W as f32, GBA_H as f32);
    let (pw, ph) = if aspect > tw / th { (tw, tw / aspect) } else { (th * aspect, th) };
    let (x0, y0) = ((tw - pw) / 2.0, (th - ph) / 2.0);
    let mut rgb = vec![0u8; (GBA_W * GBA_H * 3) as usize];
    for y in 0..GBA_H {
        let v = (y as f32 + 0.5 - y0) / ph;
        if !(0.0..1.0).contains(&v) { continue; }
        let sy = ((v * h as f32) as u32).min(h - 1);
        for x in 0..GBA_W {
            let u = (x as f32 + 0.5 - x0) / pw;
            if !(0.0..1.0).contains(&u) { continue; }
            let sx = ((u * w as f32) as u32).min(w - 1);
            let s = ((sy * w + sx) * 4) as usize;
            let d = ((y * GBA_W + x) * 3) as usize;
            rgb[d..d + 3].copy_from_slice(&[xrgb8888[s + 2], xrgb8888[s + 1], xrgb8888[s]]);
        }
    }
    encode(&rgb)
}

fn encode(rgb: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, GBA_W, GBA_H);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().ok()?;
    writer.write_image_data(rgb).ok()?;
    writer.finish().ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_frame_fits_the_thumbnail_with_bars() {
        let frame = vec![255u8; 256 * 224 * 4];
        let png = png_fit(&frame, (256, 224), 4.0 / 3.0).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buf).unwrap();
        // 4:3 in 3:2: 213 columns lit, black bars left and right.
        assert_eq!(&buf[0..3], &[0, 0, 0]);
        let mid = (80 * 240 + 120) * 3;
        assert_eq!(&buf[mid..mid + 3], &[255, 255, 255]);
    }
}
