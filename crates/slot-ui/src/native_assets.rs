//! Upscale v1.7 Slot source PNGs to KONKR texture density without altering
//! original 720x480 geometry, animation curves, ROMs, or save files.
//! The KONKR framebuffer is already native 960x640; preprocessing transparent
//! source art avoids repeated fractional bilinear sampling on display.
pub const NATIVE_W: u32 = 960;
pub const NATIVE_H: u32 = 640;

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

fn taps(input:u32,output:u32)->Vec<[Tap;4]> {
    (0..output).map(|n|{
        let pos=(n as f32+0.5)*input as f32/output as f32-0.5;
        let base=pos.floor() as i32;
        let mut out=[Tap{at:0,weight:0.0};4];
        for (i,t) in out.iter_mut().enumerate() {
            let p=base+i as i32-1;
            t.at=p.clamp(0,input as i32-1) as usize;
            t.weight=mitchell(pos-p as f32);
        }
        out
    }).collect()
}

/// Returns RGBA upload pixels and physical texture dimensions. Keep existing
/// Draw::Tex coordinates in logical units. This is filtering, NOT invented detail.
pub fn native_rgba(src:&[u8],w:u32,h:u32)->(Vec<u8>,u32,u32) {
    let Some(expected)=w.checked_mul(h).and_then(|n|n.checked_mul(4)) else {
        return (src.to_vec(),w,h);
    };
    if w==0||h==0||src.len()!=expected as usize||w>=NATIVE_W||h>=NATIVE_H {
        return (src.to_vec(),w,h);
    }
    let (tw,th)=((w*4+1)/3,(h*4+1)/3);
    if tw>NATIVE_W||th>NATIVE_H||(tw==w&&th==h) {
        return (src.to_vec(),w,h);
    }
    let (xs,ys)=(taps(w,tw),taps(h,th));
    let mut dst=vec![0u8;(tw*th*4) as usize];
    for (y,y_taps) in ys.iter().enumerate() {
        for (x,x_taps) in xs.iter().enumerate() {
            let mut accum=[0.0f32;4]; // premultiplied RGB + alpha
            for sy in y_taps {
                for sx in x_taps {
                    let k=(sy.at*w as usize+sx.at)*4;
                    let wt=sy.weight*sx.weight;
                    let alpha=src[k+3] as f32/255.0;
                    accum[3]+=wt*alpha;
                    for ch in 0..3 {accum[ch]+=wt*alpha*src[k+ch] as f32/255.0;}
                }
            }
            let k=(y*tw as usize+x)*4;
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
    (dst,tw,th)
}
