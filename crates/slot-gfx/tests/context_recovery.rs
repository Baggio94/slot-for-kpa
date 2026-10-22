#![cfg(all(target_os = "linux", feature = "test-support"))]
use slot_gfx::{Compositor, Draw, ScreenEffect, TestEgl, SRC_H, SRC_W};

#[test]
fn real_context_loss_preserves_texture_ids_pixels_and_settings_and_allows_retry() {
    let surface = TestEgl::new();
    let mut gpu = Compositor::new(&surface).unwrap();
    gpu.enable_context_recovery();
    let first = gpu.create_texture(2, 2, &[255; 16]);
    let second = gpu.create_texture_nearest(2, 2, &[127; 16]);
    gpu.update_texture(first, 1, 1, &[255, 0, 0, 255]);
    assert_eq!(gpu.recovery_bytes(), 20);
    gpu.fit((960, 640));
    gpu.set_screen_power(0.8);
    gpu.set_screen_effect(ScreenEffect::Lcd3x);
    gpu.set_blue_light(2);
    gpu.set_picture([0.1, 0.1, 0.8, 0.8]);
    gpu.set_game_source_rect([0.1, 0.1, 0.8, 0.8]);
    gpu.set_paper(1, &[128, 128, 128, 255]);
    let frame = [50, 90, 150, 255].repeat((SRC_W * SRC_H) as usize);
    let items = [
        Draw::Game,
        Draw::Tex {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
            tex: first,
            alpha: 1.,
        },
        Draw::Turned {
            x: 150.,
            y: 20.,
            w: 100.,
            h: 100.,
            tex: second,
            alpha: 0.8,
            turn: 0.2,
        },
        Draw::Shot { tex: second },
    ];
    let render = |g: &mut Compositor| {
        g.upload_game(&frame);
        g.begin_frame();
        g.draw_list(&items);
        g.read_frame()
    };
    let before = render(&mut gpu);
    assert!(before.iter().any(|v| *v != 0));
    drop(surface); // GPU object intentionally survives real context destruction.
    let replacement = TestEgl::new();
    // Force a detectable failed upload transaction, then retry from intact CPU data.
    unsafe { gl::Enable(0xffff_ffff) };
    assert!(gpu.restore_after_context_loss(&replacement).is_err());
    assert!(!gpu.graphics_ready());
    assert_eq!(gpu.recovery_bytes(), 24);
    gpu.restore_after_context_loss(&replacement).unwrap();
    assert!(gpu.graphics_ready());
    assert_eq!(before, render(&mut gpu));
    drop(replacement);
    let third = TestEgl::new();
    gpu.restore_after_context_loss(&third).unwrap();
    assert_eq!(before, render(&mut gpu));
    assert_eq!(unsafe { gl::GetError() }, gl::NO_ERROR);
    drop(gpu); // Normal destruction while its owning context is current.
}
