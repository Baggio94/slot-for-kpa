//! RA CLOCK LAB: opt-in coherent UI VSync clock; NOT a Vulkan backend.
//! Android GLSurfaceView still owns the EGL swap, and all core, audio,
//! Run-Ahead, RTC and save-state paths retain their previous behavior.
#[derive(Default, Debug)]
pub struct Clock { last_vsync_ns: Option<u64> }

impl Clock {
    pub fn reset(&mut self) { self.last_vsync_ns = None; }

    /// Return elapsed seconds using consecutive Choreographer frame stamps.
    /// Repeated stamps add zero game time; bad/missing timestamps use existing
    /// 50ms-clipped GL callback delta. A long suspend cannot create debt spikes.
    pub fn sample(&mut self, frame_ns: i64, fallback_elapsed: f64) -> f64 {
        let fallback = if fallback_elapsed.is_finite() {
            fallback_elapsed.clamp(0.0, 0.05)
        } else { 0.0 };
        if frame_ns <= 0 { self.reset(); return fallback; }
        let stamp = frame_ns as u64;
        let elapsed = match self.last_vsync_ns {
            Some(prev) if stamp == prev => 0.0,
            Some(prev) if stamp > prev && stamp - prev <= 200_000_000 =>
                ((stamp - prev) as f64 * 1e-9).min(0.05),
            _ => fallback,
        };
        self.last_vsync_ns = Some(stamp);
        elapsed
    }
}
#[cfg(test)]
mod tests {
    use super::Clock;
    use crate::settings::frame_debt_ra_clock;
    #[test]
    fn ra_clock_lab_vsync_deltas_ignore_variable_gl_callback_time() {
        let mut c=Clock::default();
        assert!((c.sample(1_000_000_000, 0.016)-0.016).abs()<1e-9);
        assert!((c.sample(1_016_666_667, 0.025)-0.016666667).abs()<1e-8);
        assert!((c.sample(1_033_333_334, 0.008)-0.016666667).abs()<1e-8);
    }
    #[test]
    fn ra_clock_lab_repeated_stamp_never_double_steps() {
        let mut c=Clock::default();c.sample(1_000_000_000,0.016);
        assert_eq!(c.sample(1_000_000_000,0.016),0.0);
    }
    #[test]
    fn ra_clock_lab_missing_backward_and_discontinuous_clock_falls_back() {
        let mut c=Clock::default();
        assert_eq!(c.sample(0,0.016),0.016);
        c.sample(1_000_000_000,0.016);
        assert_eq!(c.sample(2_000_000_000,0.015),0.015);
        assert_eq!(c.sample(1_500_000_000,0.025),0.025);
        assert_eq!(c.sample(-1,0.016),0.016);
    }
    #[test]
    fn ra_clock_lab_pause_and_invalid_deltas_are_bounded() {
        let mut c=Clock::default();
        c.sample(1_000_000_000,0.016);
        assert_eq!(c.sample(1_150_000_000,0.016),0.05);
        assert_eq!(c.sample(1_500_000_000,1.0),0.05);
        assert_eq!(c.sample(0,f64::NAN),0.0);
    }
    #[test]
    fn ra_clock_lab_fractional_60hz_to_gba_clock_keeps_one_core_frame() {
        let p=1.0/59.7275;let mut debt=0.0;let mut n=0usize;
        for _ in 0..6000 {
            debt=frame_debt_ra_clock(debt,1.0/60.0,1.0,p);
            if debt>=p {debt-=p;n+=1;}
            assert!(debt>=0.0 && debt<p);
        }
        assert!((n as f64-6000.0*59.7275/60.0).abs()<=1.0);
    }
    #[test]
    fn ra_clock_lab_long_vsync_gap_preserves_bounded_debt() {
        let p=1.0/59.7275;
        let d=frame_debt_ra_clock(0.0,0.2,1.0,p);
        assert!((d-4.0*p).abs()<1e-9);
        assert!(d-p>p);
    }
}
