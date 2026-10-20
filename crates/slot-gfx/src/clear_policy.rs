//! Conservative pure policy for the beta15 KONKR (960x640) A/B experiment.
//! Never skip the default-FBO clear when a final opaque blit does not cover
//! the complete physical panel. No OpenGL calls in this module.

pub(crate) fn skip_default_framebuffer_clear(
    window: (u32, u32),
    rect: (i32, i32, i32, i32),
    framed: bool,
    split_active: bool,
    shake: f32,
) -> bool {
    // A/B is intentionally exclusive to the one tested hardware layout.
    // Exclude transitions, shaking, margins, split/4:3/bezel and other panels.
    window == (960, 640)
        && !framed
        && !split_active
        && shake == 0.0
        && rect == (0, 0, window.0 as i32, window.1 as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f8_fullscreen_opaque_konkr_blit_allows_skip() {
        assert!(skip_default_framebuffer_clear(
            (960, 640), (0, 0, 960, 640), false, false, 0.0
        ));
    }
    #[test]
    fn f8_margin_or_shake_preserves_clear() {
        assert!(!skip_default_framebuffer_clear(
            (960, 640), (1, 0, 960, 640), false, false, 0.0
        ));
        assert!(!skip_default_framebuffer_clear(
            (960, 640), (0, 0, 960, 640), false, false, 0.01
        ));
        assert!(!skip_default_framebuffer_clear(
            (960, 640), (0, 1, 960, 639), false, false, 0.0
        ));
    }
    #[test]
    fn f8_bezel_split_and_other_panels_preserve_clear() {
        assert!(!skip_default_framebuffer_clear(
            (960, 640), (0, 0, 960, 640), true, false, 0.0
        ));
        assert!(!skip_default_framebuffer_clear(
            (960, 640), (0, 0, 960, 640), false, true, 0.0
        ));
        assert!(!skip_default_framebuffer_clear(
            (720, 480), (0, 0, 720, 480), false, false, 0.0
        ));
        assert!(!skip_default_framebuffer_clear(
            (960, 720), (0, 0, 960, 720), false, false, 0.0
        ));
    }
    #[test]
    fn f8_invalid_or_partial_coverage_preserves_clear() {
        for rect in [(0, 0, 959, 640), (0, 0, 960, 639),
                     (0, 0, 0, 640), (-1, 0, 961, 640)] {
            assert!(!skip_default_framebuffer_clear(
                (960, 640), rect, false, false, 0.0
            ));
        }
    }
}
