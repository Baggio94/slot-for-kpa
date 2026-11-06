//! Frame Blending for the cores that have no option for it (NES, SNES,
//! Mega Drive): each picture is mixed with the previous one before it is
//! uploaded, so every shader draws the mixed picture. A sprite a game shows
//! every other frame becomes a steady half-transparent one, as on the slow
//! screens of the time. The Game Boy cores blend themselves (game.rs).

/// What to mix a picture with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    /// Half the picture, half the one before.
    Mix,
    /// Half the picture, half what was last shown: a fading trail.
    Ghosting,
}

#[derive(Default)]
pub struct FrameBlend {
    /// The last picture (Mix) or the last mixed picture (Ghosting).
    prev: Vec<u8>,
    out: Vec<u8>,
    size: (u32, u32),
}

impl FrameBlend {
    /// The picture to upload for `frame` (XRGB8888 bytes, `size` pixels).
    /// The first picture, or one of a new size, is shown as it is.
    pub fn apply<'a>(&'a mut self, mode: Mode, frame: &'a [u8], size: (u32, u32)) -> &'a [u8] {
        if mode == Mode::Off {
            self.prev.clear();
            return frame;
        }
        if self.prev.len() != frame.len() || self.size != size {
            self.size = size;
            self.prev.clear();
            self.prev.extend_from_slice(frame);
            return frame;
        }
        self.out.resize(frame.len(), 0);
        for ((out, &now), &before) in self.out.iter_mut().zip(frame).zip(&self.prev) {
            *out = ((u16::from(now) + u16::from(before) + 1) / 2) as u8;
        }
        match mode {
            Mode::Ghosting => self.prev.copy_from_slice(&self.out),
            _ => self.prev.copy_from_slice(frame),
        }
        &self.out
    }

    /// Forget the last picture: the next one starts afresh (a state load,
    /// a rewind, another game).
    pub fn reset(&mut self) { self.prev.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_shows_the_picture_untouched() {
        let mut blend = FrameBlend::default();
        assert_eq!(blend.apply(Mode::Off, &[10, 20], (1, 1)), &[10, 20]);
    }

    #[test]
    fn mix_halves_a_sprite_shown_every_other_frame() {
        let mut blend = FrameBlend::default();
        let (on, off) = ([200u8, 0, 0, 0], [0u8, 0, 0, 0]);
        assert_eq!(blend.apply(Mode::Mix, &on, (1, 1)), &on, "the first picture as it is");
        assert_eq!(blend.apply(Mode::Mix, &off, (1, 1)), &[100, 0, 0, 0]);
        assert_eq!(blend.apply(Mode::Mix, &on, (1, 1)), &[100, 0, 0, 0], "steady, not flickering");
    }

    #[test]
    fn ghosting_leaves_a_fading_trail() {
        let mut blend = FrameBlend::default();
        blend.apply(Mode::Ghosting, &[200], (1, 1));
        assert_eq!(blend.apply(Mode::Ghosting, &[0], (1, 1)), &[100]);
        assert_eq!(blend.apply(Mode::Ghosting, &[0], (1, 1)), &[50]);
        assert_eq!(blend.apply(Mode::Ghosting, &[0], (1, 1)), &[25]);
    }

    #[test]
    fn a_new_size_starts_afresh() {
        let mut blend = FrameBlend::default();
        blend.apply(Mode::Mix, &[200], (1, 1));
        assert_eq!(blend.apply(Mode::Mix, &[0, 0], (2, 1)), &[0, 0]);
    }
}
