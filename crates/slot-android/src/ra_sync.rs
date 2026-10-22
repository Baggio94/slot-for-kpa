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

/// Windowed-sinc taps per output sample, as original Slot 1.8.
const TAPS: usize = 48;
const HALF: usize = TAPS / 2;
/// Sub-sample positions in the kernel table; neighbours are blended.
const PHASES: usize = 256;
/// Pass band edge relative to the lower of the two Nyquist rates: keeps
/// 22kHz at 48kHz output and folds nothing back when a core runs faster.
const CUTOFF: f64 = 22_000.0 / 24_000.0;
/// Kaiser window shape.
const BETA: f64 = 7.0;
/// Dynamic Rate Control moves the ratio by 0.5% at most; only a different
/// core rate (mGBA 32768Hz, gpSP 65536Hz) needs a new kernel.
const KERNEL_RETUNE: f64 = 0.02;

/// Interleaved stereo i16 resampler: a 48-tap Kaiser-windowed sinc, the
/// filter original Slot 1.8 uses for its 48kHz output. Unlike linear or
/// cubic interpolation it does not fold high GBA square-wave harmonics back
/// into the audible band as grit. Fed one core frame at a time without seams.
#[derive(Debug)]
pub struct Resampler {
    kernel: Vec<f32>,
    kernel_ratio: f64,
    // Interleaved L/R input frames still needed by the filter window.
    history: Vec<f32>,
    // Read position in `history`, in input frames.
    pos: f64,
}

impl Default for Resampler {
    fn default() -> Self {
        Self { kernel: Vec::new(), kernel_ratio: 0.0,
            history: vec![0.0; TAPS * 2], pos: HALF as f64 }
    }
}

impl Resampler {
    pub fn reset(&mut self) { *self = Self::default(); }

    /// Append `input` (interleaved L/R) resampled by `ratio` output frames
    /// per input frame to `out`.
    pub fn process(&mut self, input: &[i16], ratio: f64, out: &mut Vec<i16>) {
        if !(ratio > 0.0) || !ratio.is_finite() { return; }
        if self.kernel.is_empty() || (ratio / self.kernel_ratio - 1.0).abs() > KERNEL_RETUNE {
            self.kernel = kernel(CUTOFF * ratio.min(1.0));
            self.kernel_ratio = ratio;
        }
        let step = 1.0 / ratio;
        self.history.extend(input[..input.len() / 2 * 2].iter().map(|&s| f32::from(s)));
        let frames = self.history.len() / 2;
        while self.pos + (HALF as f64) < frames as f64 {
            let i = self.pos as usize;
            let at = (self.pos - i as f64) * PHASES as f64;
            let phase = (at as usize).min(PHASES - 1);
            let blend = (at - phase as f64) as f32;
            let a = &self.kernel[phase * TAPS..(phase + 1) * TAPS];
            let b = &self.kernel[(phase + 1) * TAPS..(phase + 2) * TAPS];
            let window = &self.history[(i + 1 - HALF) * 2..(i + 1 + HALF) * 2];
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for t in 0..TAPS {
                let w = a[t] + (b[t] - a[t]) * blend;
                l += window[t * 2] * w;
                r += window[t * 2 + 1] * w;
            }
            out.push(l.round().clamp(-32768.0, 32767.0) as i16);
            out.push(r.round().clamp(-32768.0, 32767.0) as i16);
            self.pos += step;
        }
        let drop = (self.pos as usize + 1).saturating_sub(HALF).min(frames);
        self.history.drain(..drop * 2);
        self.pos -= drop as f64;
    }
}

fn kernel(cutoff: f64) -> Vec<f32> {
    let mut k = vec![0.0f32; (PHASES + 1) * TAPS];
    for phase in 0..=PHASES {
        let frac = phase as f64 / PHASES as f64;
        let mut taps = [0.0f64; TAPS];
        for (t, tap) in taps.iter_mut().enumerate() {
            let x = t as f64 + 1.0 - HALF as f64 - frac;
            let window = bessel_i0(BETA * (1.0 - (x / HALF as f64).powi(2)).max(0.0).sqrt())
                / bessel_i0(BETA);
            *tap = cutoff * sinc(cutoff * x) * window;
        }
        // Unity gain at every phase: DC and slow signals pass unchanged.
        let sum: f64 = taps.iter().sum();
        for (dst, tap) in k[phase * TAPS..(phase + 1) * TAPS].iter_mut().zip(taps) {
            *dst = (tap / sum) as f32;
        }
    }
    k
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 { 1.0 } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..32 {
        term *= (x / (2.0 * k as f64)).powi(2);
        sum += term;
    }
    sum
}

/// Largest Frame Delay: half a 60Hz refresh, as RetroArch advises.
pub const FRAME_DELAY_MAX_NS: i64 = 8_000_000;
/// Time kept free before the next refresh for GPU work and the swap.
const FRAME_DELAY_SAFETY_NS: i64 = 4_000_000;
/// Refreshes measured before the delay may grow (1s at 60Hz).
const FRAME_DELAY_WINDOW: u32 = 60;
const FRAME_DELAY_STEP_NS: i64 = 1_000_000;
const FRAME_DELAY_BACKOFF_NS: i64 = 2_000_000;

/// RetroArch's automatic Frame Delay. Each display callback waits this long
/// after VSync before reading the buttons and running the core, so input is
/// sampled later for the same picture. The delay grows by 1ms every second
/// while the slowest frame still leaves `FRAME_DELAY_SAFETY_NS` free, shrinks
/// at once when the frames get slower, and backs off 2ms (then holds for 3s)
/// whenever a refresh is missed.
#[derive(Default, Debug)]
pub struct FrameDelay {
    delay_ns: i64,
    worst_cost_ns: i64,
    frames: u32,
    /// Windows left before the delay may grow again after a miss.
    hold: u32,
}

impl FrameDelay {
    pub fn reset(&mut self) { *self = Self::default(); }

    pub fn delay_ns(&self) -> i64 { self.delay_ns }

    /// `cost_ns`: time from VSync to the end of the frame's work, minus the
    /// delay itself. `missed`: a refresh went by without a frame.
    pub fn observe(&mut self, cost_ns: i64, missed: bool, period_ns: i64) {
        if missed {
            self.delay_ns = (self.delay_ns - FRAME_DELAY_BACKOFF_NS).max(0);
            self.hold = 3;
            self.frames = 0;
            self.worst_cost_ns = 0;
            return;
        }
        self.worst_cost_ns = self.worst_cost_ns.max(cost_ns.max(0));
        self.frames += 1;
        if self.frames < FRAME_DELAY_WINDOW { return; }
        let room = (period_ns - self.worst_cost_ns - FRAME_DELAY_SAFETY_NS).max(0);
        let cap = FRAME_DELAY_MAX_NS.min(period_ns / 2).min(room);
        if self.delay_ns > cap {
            self.delay_ns = cap;
        } else if self.hold == 0 {
            self.delay_ns = (self.delay_ns + FRAME_DELAY_STEP_NS).min(cap);
        } else {
            self.hold -= 1;
        }
        self.frames = 0;
        self.worst_cost_ns = 0;
    }
}

/// CLOCK_MONOTONIC in nanoseconds: the clock of Choreographer's
/// frameTimeNanos (System.nanoTime) on Android.
pub fn monotonic_ns() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: valid out-pointer; CLOCK_MONOTONIC is always available.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
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
        // The filter's look-ahead holds back at most one output frame.
        assert!((produced - input_frames * ratio).abs() <= ratio + 1.0,
            "{produced} vs {}", input_frames * ratio);
    }

    #[test]
    fn ra_sync_resampler_preserves_dc_and_channels_without_seams() {
        let mut rs = Resampler::default();
        let mut out = Vec::new();
        let chunk: Vec<i16> = (0..2 * 64).map(|n| if n % 2 == 0 { 1000 } else { -2000 }).collect();
        for _ in 0..50 { rs.process(&chunk, 1.4648, &mut out); }
        // After the filter fills (HALF frames of initial silence), DC passes
        // exactly on both channels, across every core-frame seam.
        assert!(out.len() > 4000);
        assert!(out.chunks_exact(2).skip(100).all(|f| f[0] == 1000 && f[1] == -2000));
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
        // Output frame k sits at input position k/ratio, HALF frames late:
        // the filter starts centred on its silent history.
        let mut worst = 0.0f64;
        for (k, frame) in out.chunks_exact(2).enumerate().skip(100).take(40_000) {
            let pos = k as f64 / ratio - HALF as f64;
            let ideal = 10_000.0 * (2.0 * std::f64::consts::PI * freq * pos / GBA_RATE).sin();
            worst = worst.max((f64::from(frame[0]) - ideal).abs());
        }
        assert!(worst < 30.0, "max error {worst}");
    }

    fn level_at(samples: &[i16], hz: f64, rate: f64) -> f64 {
        // Goertzel magnitude of the left channel at one frequency.
        let w = 2.0 * std::f64::consts::PI * hz / rate;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let left: Vec<f64> = samples.chunks_exact(2).map(|f| f64::from(f[0])).collect();
        for x in &left {
            let s0 = x + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).sqrt() / left.len() as f64
    }

    #[test]
    fn ra_sync_resampler_does_not_image_high_gba_tones_into_hearing() {
        // A 15kHz tone at 32768Hz has a mirror image at 17768Hz once
        // upsampled; interpolators leave it audible, the sinc removes it.
        let input: Vec<i16> = (0..32_768).flat_map(|n| {
            let v = (12_000.0 * (2.0 * std::f64::consts::PI * 15_000.0 * n as f64 / GBA_RATE).sin()) as i16;
            [v, v]
        }).collect();
        let mut rs = Resampler::default();
        let mut out = Vec::new();
        rs.process(&input, 48_000.0 / GBA_RATE, &mut out);
        let tone = level_at(&out, 15_000.0, 48_000.0);
        let image = level_at(&out, GBA_RATE - 15_000.0, 48_000.0);
        assert!(tone > 1_000.0, "tone lost: {tone}");
        assert!(image < tone / 100.0, "image only {:.1} dB down", 20.0 * (tone / image).log10());
    }

    #[test]
    fn ra_sync_resampler_handles_gpsp_rate_downsampling() {
        // gpSP reports 65536Hz: the kernel narrows so 30kHz content cannot
        // alias below the 24kHz output Nyquist.
        let input: Vec<i16> = (0..65_536).flat_map(|n| {
            let v = (12_000.0 * (2.0 * std::f64::consts::PI * 30_000.0 * n as f64 / 65_536.0).sin()) as i16;
            [v, v]
        }).collect();
        let mut rs = Resampler::default();
        let mut out = Vec::new();
        rs.process(&input, 48_000.0 / 65_536.0, &mut out);
        assert!((out.len() as f64 / 2.0 - 48_000.0).abs() < 2.0);
        let alias = level_at(&out, 48_000.0 - 30_000.0, 48_000.0);
        assert!(alias < 60.0, "30kHz folded back at level {alias}");
    }

    const P60: i64 = 16_666_667;

    #[test]
    fn ra_sync_frame_delay_grows_only_into_real_headroom() {
        let mut d = FrameDelay::default();
        // A light GBA frame: 3ms of work leaves room for the full 8ms.
        for _ in 0..FRAME_DELAY_WINDOW * 20 { d.observe(3_000_000, false, P60); }
        assert_eq!(d.delay_ns(), FRAME_DELAY_MAX_NS);
        // Run-Ahead 2 makes frames 9ms: shrink at once to 16.7 - 9 - 4 ms.
        for _ in 0..FRAME_DELAY_WINDOW { d.observe(9_000_000, false, P60); }
        assert_eq!(d.delay_ns(), P60 - 9_000_000 - 4_000_000);
        // Frames heavier than the whole budget get no delay at all.
        for _ in 0..FRAME_DELAY_WINDOW { d.observe(15_000_000, false, P60); }
        assert_eq!(d.delay_ns(), 0);
    }

    #[test]
    fn ra_sync_frame_delay_backs_off_and_holds_after_a_missed_refresh() {
        let mut d = FrameDelay::default();
        for _ in 0..FRAME_DELAY_WINDOW * 10 { d.observe(2_000_000, false, P60); }
        assert_eq!(d.delay_ns(), FRAME_DELAY_MAX_NS);
        d.observe(0, true, P60);
        assert_eq!(d.delay_ns(), FRAME_DELAY_MAX_NS - 2_000_000);
        // Held for three windows, then free to grow again.
        for _ in 0..FRAME_DELAY_WINDOW * 3 { d.observe(2_000_000, false, P60); }
        assert_eq!(d.delay_ns(), FRAME_DELAY_MAX_NS - 2_000_000);
        for _ in 0..FRAME_DELAY_WINDOW { d.observe(2_000_000, false, P60); }
        assert_eq!(d.delay_ns(), FRAME_DELAY_MAX_NS - 1_000_000);
    }

    #[test]
    fn ra_sync_frame_delay_never_exceeds_half_a_fast_refresh() {
        let mut d = FrameDelay::default();
        let p120 = 8_333_333;
        for _ in 0..FRAME_DELAY_WINDOW * 20 { d.observe(0, false, p120); }
        assert_eq!(d.delay_ns(), (p120 - 4_000_000).min(p120 / 2));
        assert!(monotonic_ns() > 0);
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
