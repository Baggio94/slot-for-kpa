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
            "Native frame phases: callbacks={} core_steps={} native_avg_us={} native_max_us={} native_over16={} core_avg_us={} core_max_us={} core_over16={} rewind_avg_us={} rewind_max_us={} battery_avg_us={} battery_max_us={} speculative_avg_us={} speculative_max_us={} upload_avg_us={} upload_max_us={} submit_avg_us={} submit_max_us={} submit_over16={} other_avg_us={} other_max_us={} native_over16_core_over={} native_over16_core_normal={} gpu_execution_and_egl_swap_not_measured=true",
            n, self.core_steps,
            self.native.mean_us(n), self.native.max_us, self.native.over_budget,
            self.core.mean_us(n), self.core.max_us, self.core.over_budget,
            self.rewind.mean_us(n), self.rewind.max_us,
            self.battery.mean_us(n), self.battery.max_us,
            self.speculative.mean_us(n), self.speculative.max_us,
            self.upload.mean_us(n), self.upload.max_us,
            self.submit.mean_us(n), self.submit.max_us, self.submit.over_budget,
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
}
