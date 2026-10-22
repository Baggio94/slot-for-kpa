//! RETROARCH pacing: the display clock owns the core, as in RetroArch with
//! VSync on and "Sync to Exact Content Framerate" off.
//!
//! One real core frame runs per display refresh (a 59.7275Hz GBA runs at
//! 60Hz, +0.46%), so no picture is ever repeated or skipped for clock drift.
//! Audio absorbs the difference: it is resampled to the output rate and the
//! ratio is nudged by at most +/-0.5% from the output buffer fill level
//! (RetroArch's Dynamic Rate Control, `audio_rate_control_delta` 0.005).

/// RetroArch `audio_rate_control_delta` default.
pub const RATE_CONTROL_DELTA: f64 = 0.005;
/// RetroArch `audio_max_timing_skew` default. Beyond this skew (for example
/// a 90Hz or 120Hz panel) the display cannot clock the core.
pub const MAX_TIMING_SKEW: f64 = 0.05;
/// Output rate of the resampled stream: Android's native mixer rate, which
/// also keeps AudioTrack on its fast path instead of a second resampler.
pub const OUTPUT_RATE: i32 = 48_000;

/// True when the display refresh is close enough to the core rate to drive
/// it directly, like RetroArch's audio_max_timing_skew check.
pub fn display_can_clock_core(display_hz: f64, core_fps: f64) -> bool {
    display_hz.is_finite() && core_fps.is_finite() && display_hz > 0.0 && core_fps > 0.0
        && ((display_hz - core_fps) / core_fps).abs() <= MAX_TIMING_SKEW
}

/// Number of core frames for this display callback, derived from VSync
/// timestamps. A repeated stamp runs nothing; one refresh runs one frame; a
/// missed refresh runs at most one extra frame so game speed is kept without
/// catch-up bursts. Missing, backwards or long gaps (pause, suspend) run one.
#[derive(Default, Debug)]
pub struct VsyncSteps { last_vsync_ns: Option<i64> }

impl VsyncSteps {
    pub fn reset(&mut self) { self.last_vsync_ns = None; }

    pub fn steps(&mut self, frame_ns: i64, display_hz: f64) -> usize {
        if frame_ns <= 0 || !(display_hz > 0.0) {
            self.reset();
            return 1;
        }
        let previous = self.last_vsync_ns.replace(frame_ns);
        let Some(previous) = previous else { return 1 };
        if frame_ns == previous { return 0; }
        if frame_ns < previous { return 1; }
        let refreshes = (frame_ns - previous) as f64 * 1e-9 * display_hz;
        if refreshes > 8.0 { return 1; }
        (refreshes.round() as usize).clamp(1, 2)
    }
}

/// RetroArch's Dynamic Rate Control adjustment. `queued` frames are waiting
/// in front of the speaker and `capacity` is the buffer they live in. The
/// target is a half-full buffer: emptier produces up to 0.5% more samples,
/// fuller produces up to 0.5% fewer.
pub fn rate_control_adjust(queued: usize, capacity: usize) -> f64 {
    if capacity == 0 { return 1.0; }
    let half = capacity as f64 / 2.0;
    let free = capacity.saturating_sub(queued.min(capacity)) as f64;
    let direction = (free - half) / half;
    1.0 + RATE_CONTROL_DELTA * direction
}

/// Interleaved stereo i16 resampler using 4-point cubic Hermite
/// interpolation. Keeps three input frames of history between calls so a
/// stream can be fed one core frame at a time without seams.
#[derive(Debug)]
pub struct Resampler {
    // Last four input frames: history[3] is the newest.
    history: [[f32; 2]; 4],
    // Fractional read position between history[1] and history[2].
    phase: f64,
    primed: usize,
}

impl Default for Resampler {
    fn default() -> Self { Self { history: [[0.0; 2]; 4], phase: 0.0, primed: 0 } }
}

impl Resampler {
    pub fn reset(&mut self) { *self = Self::default(); }

    /// Append `input` (interleaved L/R) resampled by `ratio` output frames
    /// per input frame to `out`.
    pub fn process(&mut self, input: &[i16], ratio: f64, out: &mut Vec<i16>) {
        if !(ratio > 0.0) || !ratio.is_finite() { return; }
        let step = 1.0 / ratio;
        for frame in input.chunks_exact(2) {
            self.history.rotate_left(1);
            self.history[3] = [f32::from(frame[0]), f32::from(frame[1])];
            if self.primed < 3 {
                // Start on the first real sample instead of fading in from
                // silence: emit nothing until history[1] holds input.
                self.primed += 1;
                continue;
            }
            while self.phase < 1.0 {
                let t = self.phase as f32;
                for channel in 0..2 {
                    let value = hermite(self.history[0][channel], self.history[1][channel],
                        self.history[2][channel], self.history[3][channel], t);
                    out.push(value.round().clamp(-32768.0, 32767.0) as i16);
                }
                self.phase += step;
            }
            self.phase -= 1.0;
        }
    }
}

fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

/// Output frames per input frame for a core whose audio is produced at
/// `core_rate` per emulated second while running at `display_hz` frames per
/// real second, before Dynamic Rate Control.
pub fn base_ratio(core_rate: f64, core_fps: f64, display_hz: f64) -> f64 {
    let real_input_rate = core_rate * display_hz / core_fps;
    f64::from(OUTPUT_RATE) / real_input_rate
}

#[cfg(test)]
mod tests {
    use super::*;

    const GBA_FPS: f64 = 59.7275;
    const GBA_RATE: f64 = 32_768.0;

    #[test]
    fn ra_sync_gba_on_60hz_is_within_skew_but_120hz_is_not() {
        assert!(display_can_clock_core(60.0, GBA_FPS));
        assert!(display_can_clock_core(59.94, GBA_FPS));
        assert!(!display_can_clock_core(90.0, GBA_FPS));
        assert!(!display_can_clock_core(120.0, GBA_FPS));
        assert!(!display_can_clock_core(0.0, GBA_FPS));
        assert!(!display_can_clock_core(f64::NAN, GBA_FPS));
    }

    #[test]
    fn ra_sync_one_core_frame_per_refresh_never_repeats_a_picture() {
        let mut v = VsyncSteps::default();
        let period = 1_000_000_000i64 / 60;
        let mut frames = 0;
        for i in 0..6000i64 {
            let n = v.steps(1_000_000_000 + i * period, 60.0);
            assert_eq!(n, 1, "refresh {i}");
            frames += n;
        }
        // Unlike a 59.7275Hz debt clock, 6000 refreshes give 6000 frames.
        assert_eq!(frames, 6000);
    }

    #[test]
    fn ra_sync_repeated_missed_and_bad_stamps_are_bounded() {
        let mut v = VsyncSteps::default();
        let p = 16_666_667i64;
        assert_eq!(v.steps(1_000_000_000, 60.0), 1);
        assert_eq!(v.steps(1_000_000_000, 60.0), 0);
        assert_eq!(v.steps(1_000_000_000 + 2 * p, 60.0), 2);
        assert_eq!(v.steps(1_000_000_000 + 6 * p, 60.0), 2);
        assert_eq!(v.steps(1_000_000_000 + 100 * p, 60.0), 1);
        assert_eq!(v.steps(500_000_000, 60.0), 1);
        assert_eq!(v.steps(0, 60.0), 1);
        assert_eq!(v.steps(1_000_000_000, 0.0), 1);
        // Jitter around one refresh still means one frame.
        assert_eq!(v.steps(1_000_000_000 + p + p / 3, 60.0), 1);
    }

    #[test]
    fn ra_sync_rate_control_matches_retroarch_direction_and_bounds() {
        assert!((rate_control_adjust(500, 1000) - 1.0).abs() < 1e-12);
        assert!((rate_control_adjust(0, 1000) - 1.005).abs() < 1e-12);
        assert!((rate_control_adjust(1000, 1000) - 0.995).abs() < 1e-12);
        assert!((rate_control_adjust(5000, 1000) - 0.995).abs() < 1e-12);
        assert_eq!(rate_control_adjust(10, 0), 1.0);
    }

    #[test]
    fn ra_sync_base_ratio_absorbs_display_skew() {
        // At 60Hz the GBA produces 32768*60/59.7275 input frames per second.
        let r = base_ratio(GBA_RATE, GBA_FPS, 60.0);
        let expected = 48_000.0 / (GBA_RATE * 60.0 / GBA_FPS);
        assert!((r - expected).abs() < 1e-12);
        assert!(r < 48_000.0 / GBA_RATE);
    }

    #[test]
    fn ra_sync_resampler_produces_ratio_times_input_frames() {
        let mut rs = Resampler::default();
        let input: Vec<i16> = (0..2 * 548).map(|n| (n % 200) as i16).collect();
        let ratio = base_ratio(GBA_RATE, GBA_FPS, 60.0);
        let mut out = Vec::new();
        for _ in 0..600 { rs.process(&input, ratio, &mut out); }
        let input_frames = 548.0 * 600.0;
        let produced = out.len() as f64 / 2.0;
        assert_eq!(out.len() % 2, 0);
        // Only the three priming frames of history are not emitted.
        assert!((produced - input_frames * ratio).abs() <= 3.0 * ratio + 1.0,
            "{produced} vs {}", input_frames * ratio);
    }

    #[test]
    fn ra_sync_resampler_preserves_dc_and_channels_without_seams() {
        let mut rs = Resampler::default();
        let mut out = Vec::new();
        let chunk: Vec<i16> = (0..2 * 64).map(|n| if n % 2 == 0 { 1000 } else { -2000 }).collect();
        for _ in 0..50 { rs.process(&chunk, 1.4648, &mut out); }
        assert!(out.chunks_exact(2).all(|f| f[0] == 1000 && f[1] == -2000));
    }

    #[test]
    fn ra_sync_resampler_tracks_a_sine_closely() {
        let mut rs = Resampler::default();
        let freq = 440.0;
        let input: Vec<i16> = (0..32_768).flat_map(|n| {
            let v = (10_000.0 * (2.0 * std::f64::consts::PI * freq * n as f64 / GBA_RATE).sin()) as i16;
            [v, v]
        }).collect();
        let ratio = 48_000.0 / GBA_RATE;
        let mut out = Vec::new();
        rs.process(&input, ratio, &mut out);
        // Output frame k sits at input position k/ratio, offset by the one
        // frame of look-behind the interpolator needs.
        let mut worst = 0.0f64;
        for (k, frame) in out.chunks_exact(2).enumerate().skip(10).take(40_000) {
            let pos = k as f64 / ratio + 1.0;
            let ideal = 10_000.0 * (2.0 * std::f64::consts::PI * freq * pos / GBA_RATE).sin();
            worst = worst.max((f64::from(frame[0]) - ideal).abs());
        }
        assert!(worst < 30.0, "max error {worst}");
    }

    #[test]
    fn ra_sync_closed_loop_settles_near_half_full_buffer() {
        // Simulate 60Hz frames feeding a 48kHz speaker through DRC while the
        // speaker clock runs 0.2% fast: the queue must neither drain nor grow.
        let capacity = 4096usize;
        let mut queued = capacity as f64 / 2.0;
        let mut rs = Resampler::default();
        let frame: Vec<i16> = vec![0; 2 * 549];
        let base = base_ratio(GBA_RATE, GBA_FPS, 60.0);
        let mut out = Vec::new();
        let speaker_per_frame = 48_000.0 * 1.002 / 60.0;
        for _ in 0..60 * 120 {
            out.clear();
            let adjust = rate_control_adjust(queued as usize, capacity);
            rs.process(&frame[..2 * 548], base * adjust, &mut out);
            queued += out.len() as f64 / 2.0;
            queued = (queued - speaker_per_frame).max(0.0);
            assert!(queued > 200.0 && queued < capacity as f64 - 200.0, "{queued}");
        }
    }
}
