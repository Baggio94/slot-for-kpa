//! RetroArch "Preemptive Frames" bookkeeping.
//!
//! The core keeps the state it had at the start of each of the last N real
//! frames. Those N frames all ran with the same buttons. When the buttons
//! change, the core goes back N frames and replays them with the new
//! buttons before running the current frame, so the game reacts N frames
//! earlier. Unlike Run-Ahead, nothing is predicted: every picture shown is
//! a real frame, and the rollback only happens when the input changes.

/// At most two frames, like Run-Ahead.
pub const MAX_FRAMES: usize = 2;

#[derive(Default)]
pub struct Preempt {
    frames: usize,
    /// Oldest first starting at `head`; buffers are reused.
    states: [Vec<u8>; MAX_FRAMES],
    head: usize,
    len: usize,
    /// Buttons and turbo every frame in the window ran with.
    input: Option<(u16, bool)>,
}

impl Preempt {
    pub fn frames(&self) -> usize { self.frames }

    /// Change the window size; a different size starts an empty window.
    pub fn set_frames(&mut self, frames: usize) {
        let frames = frames.min(MAX_FRAMES);
        if frames != self.frames {
            self.frames = frames;
            self.clear();
        }
    }

    /// Forget the window after any jump in the game's timeline (state load,
    /// rewind, undo, Link) so a rollback can never cross it.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
        self.input = None;
    }

    /// True when a full window ran with other buttons than `input`.
    pub fn needs_rollback(&self, input: (u16, bool)) -> bool {
        self.frames > 0 && self.len == self.frames
            && self.input.is_some_and(|ran| ran != input)
    }

    /// State at the start of the `k`-th frame of the window, oldest first.
    pub fn state(&self, k: usize) -> &[u8] {
        &self.states[(self.head + k) % self.frames]
    }

    pub fn state_mut(&mut self, k: usize) -> &mut Vec<u8> {
        &mut self.states[(self.head + k) % self.frames]
    }

    /// Buffer for the state at the start of the frame about to run with
    /// `input`, dropping the oldest frame once the window is full.
    pub fn push(&mut self, input: (u16, bool)) -> &mut Vec<u8> {
        self.input = Some(input);
        let slot = if self.len < self.frames {
            self.len += 1;
            (self.head + self.len - 1) % self.frames
        } else {
            let oldest = self.head;
            self.head = (self.head + 1) % self.frames;
            oldest
        };
        &mut self.states[slot]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(p: &mut Preempt, input: (u16, bool), frame: u8) {
        *p.push(input) = vec![frame];
    }

    #[test]
    fn preempt_window_keeps_the_last_frames_oldest_first() {
        let mut p = Preempt::default();
        p.set_frames(2);
        fill(&mut p, (1, false), 10);
        assert!(!p.needs_rollback((2, false)), "window not full yet");
        fill(&mut p, (1, false), 11);
        fill(&mut p, (1, false), 12);
        assert_eq!((p.state(0), p.state(1)), (&[11u8][..], &[12u8][..]));
        assert!(!p.needs_rollback((1, false)));
        assert!(p.needs_rollback((2, false)));
        assert!(p.needs_rollback((1, true)), "turbo changes the input too");
    }

    #[test]
    fn preempt_window_is_forgotten_on_clear_or_size_change() {
        let mut p = Preempt::default();
        p.set_frames(1);
        fill(&mut p, (1, false), 1);
        assert!(p.needs_rollback((2, false)));
        p.clear();
        assert!(!p.needs_rollback((2, false)));
        fill(&mut p, (1, false), 2);
        p.set_frames(1);
        assert!(p.needs_rollback((2, false)), "same size keeps the window");
        p.set_frames(2);
        assert!(!p.needs_rollback((2, false)));
        p.set_frames(0);
        assert!(!p.needs_rollback((2, false)));
    }
}
