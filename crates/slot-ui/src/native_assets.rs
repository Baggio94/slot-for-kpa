//! Upscale v1.7 Slot source PNGs to KONKR texture density without altering
//! original 720x480 geometry, animation curves, ROMs, or save files.
//! The KONKR framebuffer is already native 960x640; preprocessing transparent
//! source art avoids repeated fractional bilinear sampling on display.
pub const NATIVE_W: u32 = 960;
pub const NATIVE_H: u32 = 640;

/// Panel texels per Slot pixel: the panel density once the runtime sets it,
/// otherwise the KONKR's 4/3.
fn native_scale() -> f32 {
    let s = crate::density::scale();
    if s > 1.0 { s } else { 4.0 / 3.0 }
}

/// Full-canvas texture size at panel density (960x640 on the KONKR).
pub fn native_w() -> u32 {
    (slot_gfx::out_w() as f32 * native_scale()).round() as u32
}

pub fn native_h() -> u32 {
    (slot_gfx::OUT_H as f32 * native_scale()).round() as u32
}

#[derive(Clone, Copy)]
struct Tap { at: usize, weight: f32 }

fn mitchell(x: f32) -> f32 {
    // Mitchell-Netravali, B=C=1/3.
    const B: f32 = 1.0 / 3.0;
    const C: f32 = 1.0 / 3.0;
    let a=x.abs();
    if a<1.0 {
        ((12.0-9.0*B-6.0*C)*a*a*a+(-18.0+12.0*B+6.0*C)*a*a+6.0-2.0*B)/6.0
    } else if a<2.0 {
        ((-B-6.0*C)*a*a*a+(6.0*B+30.0*C)*a*a+(-12.0*B-48.0*C)*a+8.0*B+24.0*C)/6.0
    } else { 0.0 }
}

/// Each output pixel's source taps. Enlarging uses Mitchell's four taps;
/// reducing widens the kernel by the ratio, so a 3x master shrunk to the
/// KONKR's 4/3 averages every source pixel instead of skipping some.
fn taps(input:u32,output:u32)->Vec<Vec<Tap>> {
    let ratio=(input as f32/output as f32).max(1.0);
    (0..output).map(|n|{
        let pos=(n as f32+0.5)*input as f32/output as f32-0.5;
        let reach=2.0*ratio;
        let first=(pos-reach).ceil() as i32;
        let last=(pos+reach).floor() as i32;
        let mut out:Vec<Tap>=(first..=last).map(|p|Tap{
            at:p.clamp(0,input as i32-1) as usize,
            weight:mitchell((pos-p as f32)/ratio),
        }).filter(|t|t.weight!=0.0).collect();
        let sum:f32=out.iter().map(|t|t.weight).sum();
        if sum!=0.0 { for t in &mut out { t.weight/=sum; } }
        out
    }).collect()
}

/// Returns RGBA upload pixels and physical texture dimensions. Keep existing
/// Draw::Tex coordinates in logical units. This is filtering, NOT invented detail.
pub fn native_rgba(src:&[u8],w:u32,h:u32)->(Vec<u8>,u32,u32) {
    let Some(expected)=w.checked_mul(h).and_then(|n|n.checked_mul(4)) else {
        return (src.to_vec(),w,h);
    };
    let (nw,nh)=(native_w().max(NATIVE_W),native_h().max(NATIVE_H));
    if w==0||h==0||src.len()!=expected as usize||w>=nw||h>=nh {
        return (src.to_vec(),w,h);
    }
    let s=native_scale() as f64;
    let (tw,th)=((w as f64*s).round() as u32,(h as f64*s).round() as u32);
    if tw>nw||th>nh||(tw==w&&th==h) {
        return (src.to_vec(),w,h);
    }
    (resample(src,w,h,tw,th),tw,th)
}

/// A PNG master filtered straight to `tw` x `th`, whatever its own size: a
/// 3x master is reduced, a 1x one enlarged, in one Mitchell pass.
pub fn png_at(bytes: &[u8], tw: u32, th: u32) -> Vec<u8> {
    let (rgba, sw, sh) = crate::art::decode_png_bytes(bytes).expect("a bundled png decodes");
    resample(&rgba, sw, sh, tw, th)
}

/// A PNG master for a `w` x `h` (logical) layer at panel density, like
/// `native_rgba` but filtered from the master's own pixels.
pub fn native_png(bytes: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let s = native_scale() as f64;
    let (tw, th) = ((w as f64 * s).round() as u32, (h as f64 * s).round() as u32);
    (png_at(bytes, tw, th), tw, th)
}

/// Mitchell-filter `src` (`w` x `h`) to `tw` x `th`, alpha-correctly.
pub(crate) fn resample(src:&[u8],w:u32,h:u32,tw:u32,th:u32)->Vec<u8> {
    if (tw,th)==(w,h) || src.len()!=(w*h*4) as usize {
        return src.to_vec();
    }
    let (xs,ys)=(taps(w,tw),taps(h,th));
    let (w,tw,th)=(w as usize,tw as usize,th as usize);
    // Premultiplied RGB + alpha, filtered across then down.
    let pre:Vec<[f32;4]>=src.chunks_exact(4).map(|p|{
        let a=p[3] as f32/255.0;
        [a*p[0] as f32/255.0,a*p[1] as f32/255.0,a*p[2] as f32/255.0,a]
    }).collect();
    let mut across=vec![[0.0f32;4];tw*h as usize];
    for y in 0..h as usize {
        let row=&pre[y*w..(y+1)*w];
        for (x,x_taps) in xs.iter().enumerate() {
            let mut acc=[0.0f32;4];
            for t in x_taps {
                let p=row[t.at];
                for c in 0..4 {acc[c]+=t.weight*p[c];}
            }
            across[y*tw+x]=acc;
        }
    }
    let mut dst=vec![0u8;tw*th*4];
    for (y,y_taps) in ys.iter().enumerate() {
        for x in 0..tw {
            let mut accum=[0.0f32;4];
            for t in y_taps {
                let p=across[t.at*tw+x];
                for c in 0..4 {accum[c]+=t.weight*p[c];}
            }
            let k=(y*tw+x)*4;
            let alpha=accum[3].clamp(0.0,1.0);
            dst[k+3]=(alpha*255.0).round() as u8;
            // Even alpha=1/255 still needs its premultiplied hue.
            // The old threshold produced a dark fringe on clear GBC shells.
            if dst[k+3]>0 && accum[3]>0.0 {
                for ch in 0..3 {
                    dst[k+ch]=((accum[ch]/accum[3]).clamp(0.0,1.0)*255.0).round() as u8;
                }
            }
        }
    }
    dst
}

/// Refit a full-width slot band (`tw` x `th` texels, mouth centred) to `out`
/// texels wide: the ends and the mouth stay as drawn, the housing between
/// them is repeated or shortened. Identity when `out == tw`.
pub fn refit_band(src: &[u8], tw: u32, th: u32, out: u32) -> Vec<u8> {
    let s = tw as f32 / slot_gfx::BASE_W as f32;
    let end = (40.0 * s).round() as usize;
    let core = (160.0 * s).round() as usize;
    let (tw, th, out) = (tw as usize, th as usize, out as usize);
    if out == tw || src.len() != tw * th * 4 || out < 2 * (end + core) || tw < 2 * (end + core) {
        return src.to_vec();
    }
    let (c, oc) = (tw / 2, out / 2);
    let (left, right) = (c - core, c + core);
    let period = left - end;
    let column = |x: usize| -> usize {
        if x < end {
            x
        } else if x < oc - core {
            left - 1 - (oc - core - 1 - x) % period
        } else if x < oc + core {
            left + (x - (oc - core))
        } else if x < out - end {
            right + (x - (oc + core)) % (tw - end - right)
        } else {
            tw - (out - x)
        }
    };
    let map: Vec<usize> = (0..out).map(column).collect();
    let mut dst = vec![0u8; out * th * 4];
    for y in 0..th {
        for (x, &from) in map.iter().enumerate() {
            let (d, s) = ((y * out + x) * 4, (y * tw + from) * 4);
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    dst
}

#[cfg(test)]
mod band_tests {
    use super::refit_band;

    #[test]
    fn a_band_keeps_its_mouth_and_ends_at_any_width() {
        let (tw, th) = (960u32, 2u32);
        let src: Vec<u8> = (0..tw * th).flat_map(|i| {
            let x = (i % tw) as u16;
            [(x & 0xff) as u8, (x >> 8) as u8, 0, 255]
        }).collect();
        assert_eq!(refit_band(&src, tw, th, tw), src);
        let col = |px: &[u8], _w: u32, x: u32| u16::from_le_bytes([px[(x * 4) as usize], px[(x * 4 + 1) as usize]]);
        for out in [1138u32, 853 * 4 / 3, 640, 736] {
            let o = refit_band(&src, tw, th, out);
            assert_eq!(o.len(), (out * th * 4) as usize);
            assert_eq!(col(&o, out, 0), 0);
            assert_eq!(col(&o, out, out - 1), 959);
            assert_eq!(col(&o, out, out / 2), 480, "mouth centred at {out}");
            assert_eq!(col(&o, out, out / 2 - 213), 480 - 213);
        }
    }
}
