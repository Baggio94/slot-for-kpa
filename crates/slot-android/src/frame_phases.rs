//! Beta11: read-only 5-second breakdown of Android native draw callback work.
//! Both the Linux host tests and Android production use this exact aggregator.
//! Times are CPU-thread wall durations, including waits, not GPU execution,
//! eglSwapBuffers, physical present timestamps or controller-to-photon latency.
use std::time::{Duration, Instant};

pub(crate) const PHASE_LOG_PERIOD: Duration = Duration::from_secs(5);
pub(crate) const SLOW_GL_BUDGET_US: u64 = 16_667;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct FramePhaseSample {
    pub core_us: u64,
    pub rewind_work_us: u64,
    pub periodic_battery_us: u64,
    pub speculative_us: u64,
    pub gpu_upload_us: u64,
    pub gpu_submit_us: u64,
    // Beta12: nested subsets of gpu_submit_us. Excluded from accounted_us
    // to prevent double-counting the same wall duration.
    pub submit_setup_us: u64,
    pub submit_build_us: u64,
    pub submit_draw_us: u64,
    pub submit_blit_us: u64,
    // Beta13: nested inside submit_blit/end_frame. Never count twice.
    pub end_setup_us: u64,
    // Nested CPU thread wall times inside end_setup; no double counting.
    pub end_geometry_us: u64,
    pub end_bind_us: u64,
    pub end_viewport_us: u64,
    pub end_clear_us: u64,
    pub end_state_us: u64,
    pub end_quad_us: u64,
    pub end_bezel_us: u64,
    pub native_total_us: u64,
    pub core_steps: u32,
}

impl FramePhaseSample {
    pub(crate) fn accounted_us(self) -> u64 {
        self.core_us.saturating_add(self.rewind_work_us)
            .saturating_add(self.periodic_battery_us)
            .saturating_add(self.speculative_us)
            .saturating_add(self.gpu_upload_us)
            .saturating_add(self.gpu_submit_us)
    }
    pub(crate) fn other_us(self) -> u64 {
        self.native_total_us.saturating_sub(self.accounted_us())
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct PhaseCounter {
    pub total_us: u64,
    pub max_us: u64,
    pub over_budget: u32,
}
impl PhaseCounter {
    pub(crate) fn add(&mut self, elapsed_us: u64) {
        self.total_us = self.total_us.saturating_add(elapsed_us);
        self.max_us = self.max_us.max(elapsed_us);
        if elapsed_us > SLOW_GL_BUDGET_US {
            self.over_budget = self.over_budget.saturating_add(1);
        }
    }
    pub(crate) fn mean_us(self, callbacks: u32) -> u64 {
        if callbacks == 0 { 0 } else { self.total_us / u64::from(callbacks) }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct PhaseSummary {
    pub callbacks: u32,
    pub core_steps: u64,
    pub native: PhaseCounter,
    pub core: PhaseCounter,
    pub rewind: PhaseCounter,
    pub battery: PhaseCounter,
    pub speculative: PhaseCounter,
    pub upload: PhaseCounter,
    pub submit: PhaseCounter,
    pub submit_setup: PhaseCounter,
    pub submit_build: PhaseCounter,
    pub submit_draw: PhaseCounter,
    pub submit_blit: PhaseCounter,
    pub end_setup: PhaseCounter,
    pub end_geometry: PhaseCounter,
    pub end_bind: PhaseCounter,
    pub end_viewport: PhaseCounter,
    pub end_clear: PhaseCounter,
    pub end_state: PhaseCounter,
    pub end_quad: PhaseCounter,
    pub end_bezel: PhaseCounter,
    pub other: PhaseCounter,
    /// Native >16.667 ms, with inner core processing also >16.667 ms.
    pub slow_with_core_over: u32,
    /// Native >16.667 ms, but core processing stayed within one refresh.
    pub slow_with_core_normal: u32,
}
impl PhaseSummary {
    pub(crate) fn add(&mut self, frame: FramePhaseSample) {
        self.callbacks = self.callbacks.saturating_add(1);
        self.core_steps = self.core_steps.saturating_add(u64::from(frame.core_steps));
        self.native.add(frame.native_total_us);
        self.core.add(frame.core_us);
        self.rewind.add(frame.rewind_work_us);
        self.battery.add(frame.periodic_battery_us);
        self.speculative.add(frame.speculative_us);
        self.upload.add(frame.gpu_upload_us);
        self.submit.add(frame.gpu_submit_us);
        self.submit_setup.add(frame.submit_setup_us);
        self.submit_build.add(frame.submit_build_us);
        self.submit_draw.add(frame.submit_draw_us);
        self.submit_blit.add(frame.submit_blit_us);
        self.end_setup.add(frame.end_setup_us);
        self.end_geometry.add(frame.end_geometry_us);
        self.end_bind.add(frame.end_bind_us);
        self.end_viewport.add(frame.end_viewport_us);
        self.end_clear.add(frame.end_clear_us);
        self.end_state.add(frame.end_state_us);
        self.end_quad.add(frame.end_quad_us);
        self.end_bezel.add(frame.end_bezel_us);
        self.other.add(frame.other_us());
        if frame.native_total_us > SLOW_GL_BUDGET_US {
            if frame.core_us > SLOW_GL_BUDGET_US {
                self.slow_with_core_over = self.slow_with_core_over.saturating_add(1);
            } else {
                self.slow_with_core_normal = self.slow_with_core_normal.saturating_add(1);
            }
        }
    }
    pub(crate) fn report(self) -> String {
        let n = self.callbacks;
        format!(
            "Native frame phases: callbacks={} core_steps={} native_avg_us={} native_max_us={} native_over16={} core_avg_us={} core_max_us={} core_over16={} rewind_avg_us={} rewind_max_us={} battery_avg_us={} battery_max_us={} speculative_avg_us={} speculative_max_us={} upload_avg_us={} upload_max_us={} submit_avg_us={} submit_max_us={} submit_over16={} submit_setup_avg_us={} submit_setup_max_us={} submit_build_avg_us={} submit_build_max_us={} submit_draw_avg_us={} submit_draw_max_us={} submit_draw_over16={} submit_blit_avg_us={} submit_blit_max_us={} submit_blit_over16={} end_setup_avg_us={} end_setup_max_us={} end_setup_over16={} end_geometry_avg_us={} end_geometry_max_us={} end_bind_avg_us={} end_bind_max_us={} end_bind_over16={} end_viewport_avg_us={} end_viewport_max_us={} end_clear_avg_us={} end_clear_max_us={} end_clear_over16={} end_state_avg_us={} end_state_max_us={} end_state_over16={} end_quad_avg_us={} end_quad_max_us={} end_quad_over16={} end_bezel_avg_us={} end_bezel_max_us={} other_avg_us={} other_max_us={} native_over16_core_over={} native_over16_core_normal={} gpu_execution_and_egl_swap_not_measured=true",
            n, self.core_steps,
            self.native.mean_us(n), self.native.max_us, self.native.over_budget,
            self.core.mean_us(n), self.core.max_us, self.core.over_budget,
            self.rewind.mean_us(n), self.rewind.max_us,
            self.battery.mean_us(n), self.battery.max_us,
            self.speculative.mean_us(n), self.speculative.max_us,
            self.upload.mean_us(n), self.upload.max_us,
            self.submit.mean_us(n), self.submit.max_us, self.submit.over_budget,
            self.submit_setup.mean_us(n), self.submit_setup.max_us,
            self.submit_build.mean_us(n), self.submit_build.max_us,
            self.submit_draw.mean_us(n), self.submit_draw.max_us, self.submit_draw.over_budget,
            self.submit_blit.mean_us(n), self.submit_blit.max_us, self.submit_blit.over_budget,
            self.end_setup.mean_us(n), self.end_setup.max_us, self.end_setup.over_budget,
            self.end_geometry.mean_us(n), self.end_geometry.max_us,
            self.end_bind.mean_us(n), self.end_bind.max_us, self.end_bind.over_budget,
            self.end_viewport.mean_us(n), self.end_viewport.max_us,
            self.end_clear.mean_us(n), self.end_clear.max_us, self.end_clear.over_budget,
            self.end_state.mean_us(n), self.end_state.max_us, self.end_state.over_budget,
            self.end_quad.mean_us(n), self.end_quad.max_us, self.end_quad.over_budget,
            self.end_bezel.mean_us(n), self.end_bezel.max_us,
            self.other.mean_us(n), self.other.max_us,
            self.slow_with_core_over, self.slow_with_core_normal,
        )
    }
}

pub(crate) struct PhaseWindow {
    started: Instant,
    summary: PhaseSummary,
}
impl PhaseWindow {
    pub(crate) fn new() -> Self {
        Self { started: Instant::now(), summary: PhaseSummary::default() }
    }
    /// Never format or call Android logging from this hot-path aggregator.
    /// The native GL owner logs at most once per window.
    pub(crate) fn add(&mut self, sample: FramePhaseSample) -> Option<PhaseSummary> {
        self.summary.add(sample);
        if self.started.elapsed() < PHASE_LOG_PERIOD { return None; }
        self.started = Instant::now();
        Some(std::mem::take(&mut self.summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f8_phase_residual_never_underflows_when_elapsed_clock_is_shorter() {
        let frame = FramePhaseSample {
            core_us: 100, rewind_work_us: 20, gpu_submit_us: 40,
            native_total_us: 159, ..Default::default()
        };
        assert_eq!(frame.accounted_us(), 160);
        assert_eq!(frame.other_us(), 0);
    }

    #[test]
    fn f8_phase_all_categories_and_core_steps_are_accounted_once() {
        let frame = FramePhaseSample {
            core_us: 2000, rewind_work_us: 300, periodic_battery_us: 100,
            speculative_us: 600, gpu_upload_us: 400, gpu_submit_us: 1500,
            native_total_us: 6000, core_steps: 1,
            ..Default::default()
        };
        assert_eq!(frame.other_us(), 1100);
        let mut s = PhaseSummary::default();
        s.add(frame);
        s.add(frame);
        assert_eq!(s.callbacks, 2);
        assert_eq!(s.core_steps, 2);
        assert_eq!(s.other.total_us, 2200);
        assert_eq!(s.core.mean_us(2), 2000);
        assert!(s.report().contains("gpu_execution_and_egl_swap_not_measured=true"));
    }

    #[test]
    fn f8_phase_slow_native_with_fast_core_stays_distinct_from_core_stall() {
        let mut s = PhaseSummary::default();
        s.add(FramePhaseSample { native_total_us: 20_000,
            core_us: 3_000, gpu_submit_us: 17_000, ..Default::default() });
        s.add(FramePhaseSample { native_total_us: 28_000,
            core_us: 18_000, gpu_submit_us: 1_000, ..Default::default() });
        assert_eq!(s.native.over_budget, 2);
        assert_eq!(s.core.over_budget, 1);
        assert_eq!(s.submit.over_budget, 1);
        assert_eq!(s.slow_with_core_normal, 1);
        assert_eq!(s.slow_with_core_over, 1);
    }

    #[test]
    fn f8_phase_budget_boundary_and_saturated_total() {
        let mut counter = PhaseCounter::default();
        counter.add(16_667);
        counter.add(16_668);
        assert_eq!(counter.over_budget, 1);
        counter.add(u64::MAX);
        assert_eq!(counter.total_us, u64::MAX);
        assert_eq!(counter.max_us, u64::MAX);
    }

    #[test]
    fn f8_phase_window_is_quiet_before_five_seconds() {
        let mut window = PhaseWindow::new();
        assert!(window.add(FramePhaseSample { core_us: 200,
            native_total_us: 300, ..Default::default() }).is_none());
    }
    #[test]
    fn f8_phase_nested_gl_subtimes_never_double_count_submit_or_residual() {
        let sample = FramePhaseSample {
            core_us: 2_000, gpu_submit_us: 8_000, native_total_us: 11_000,
            submit_setup_us: 300, submit_build_us: 700,
            submit_draw_us: 4_000, submit_blit_us: 3_000,
            ..Default::default()
        };
        assert_eq!(sample.accounted_us(), 10_000);
        assert_eq!(sample.other_us(), 1_000);
        let mut s = PhaseSummary::default();
        s.add(sample);
        assert_eq!(s.submit.total_us, 8_000);
        assert_eq!(s.submit_draw.total_us, 4_000);
        assert_eq!(s.submit_blit.total_us, 3_000);
        assert_eq!(s.other.total_us, 1_000);
        assert!(s.report().contains("submit_blit_avg_us=3000"));
    }

    #[test]
    fn f8_phase_nested_end_frame_subphases_never_double_count_blit() {
        let sample = FramePhaseSample {
            native_total_us: 15_000, core_us: 2_000,
            gpu_submit_us: 10_000, submit_blit_us: 9_500,
            end_setup_us: 7_000, end_quad_us: 2_500, end_bezel_us: 0,
            ..Default::default()
        };
        assert_eq!(sample.accounted_us(), 12_000);
        assert_eq!(sample.other_us(), 3_000);
        let mut summary = PhaseSummary::default();
        summary.add(sample);
        assert_eq!(summary.submit_blit.total_us, 9_500);
        assert_eq!(summary.end_setup.total_us, 7_000);
        assert_eq!(summary.end_quad.total_us, 2_500);
        assert_eq!(summary.end_bezel.total_us, 0);
        assert_eq!(summary.other.total_us, 3_000);
        assert!(summary.report().contains("end_setup_avg_us=7000"));
    }

    #[test]
    fn f8_phase_beta14_end_setup_gl_calls_are_nested_not_new_native_cost() {
        let sample = FramePhaseSample {
            native_total_us: 18_000, core_us: 2_000,
            gpu_submit_us: 11_000, submit_blit_us: 10_500,
            end_setup_us: 10_000, end_quad_us: 500,
            end_geometry_us: 20, end_bind_us: 9_200,
            end_viewport_us: 5, end_clear_us: 750, end_state_us: 25,
            ..Default::default()
        };
        assert_eq!(sample.accounted_us(), 13_000);
        assert_eq!(sample.other_us(), 5_000);
        let mut summary = PhaseSummary::default();
        summary.add(sample);
        assert_eq!(summary.end_setup.total_us, 10_000);
        assert_eq!(summary.end_bind.total_us, 9_200);
        assert_eq!(summary.end_clear.total_us, 750);
        assert_eq!(summary.end_geometry.total_us, 20);
        assert_eq!(summary.other.total_us, 5_000);
        assert!(summary.report().contains("end_bind_avg_us=9200"));
        assert!(summary.report().contains("end_clear_avg_us=750"));
    }

}
