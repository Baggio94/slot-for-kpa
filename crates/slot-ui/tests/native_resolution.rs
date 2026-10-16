use slot_ui::{native_rgba,NATIVE_W,NATIVE_H};

#[test]
fn source_mould_dimensions_expand_to_native_konkr_without_resizing_draw_rects() {
    for (w,h,ww,hh) in [(360,212,480,283),(312,337,416,449),(720,58,960,77)] {
        let src=vec![99u8;(w*h*4) as usize];
        let (out,x,y)=native_rgba(&src,w,h);
        assert_eq!((x,y),(ww,hh));
        assert_eq!(out.len(),(ww*hh*4) as usize);
        assert!(out.iter().all(|&c| c==99));
    }
}
#[test]
fn already_native_fullscreen_is_preserved_byte_for_byte() {
    assert_eq!((NATIVE_W,NATIVE_H),(960,640));
    let src=vec![127;(NATIVE_W*NATIVE_H*4) as usize];
    assert_eq!(native_rgba(&src,NATIVE_W,NATIVE_H),(src,NATIVE_W,NATIVE_H));
}
#[test]
fn fully_transparent_white_does_not_pollute_coloured_edges() {
    let mut src=vec![255u8;3*3*4];
    for px in src.chunks_exact_mut(4) {px[3]=0;}
    let at=(1*3+1)*4;
    src[at..at+4].copy_from_slice(&[210,0,0,255]);
    let (out,w,h)=native_rgba(&src,3,3);
    assert_eq!((w,h),(4,4));
    for px in out.chunks_exact(4) {
        if px[3]!=0 {
            assert_eq!(px[1],0);assert_eq!(px[2],0);
            assert!((px[0] as i32-210).abs()<=1);
        }
    }
}
#[test]
fn bad_buffers_and_zero_dimensions_are_safe() {
    let src=[1u8,2,3];
    assert_eq!(native_rgba(&src,720,480).0,src);
    assert_eq!(native_rgba(&src,0,0).0,src);
}
