//! Read-only presentation accounting shared by Android EGL and host tests.
//! No rendering, emulation or audio policy belongs here.
use std::collections::VecDeque;
use std::time::Instant;

const MAX_PENDING: usize = 12;
const MAX_VALID_GAP_NS: i64 = 1_000_000_000;

#[derive(Debug, Copy, Clone)]
pub(crate) enum Timestamp {
    Pending,
    Invalid,
    Presented(i64),
}

pub(crate) trait Source {
    fn timestamp(&mut self, id: u64) -> Result<Timestamp, ()>;
    fn next_id(&mut self) -> Result<u64, ()>;
}

pub(crate) struct PresentationProbe {
    // EGL frame ID, tick at capture, and the CLOCK_MONOTONIC time of the
    // button press this frame was the first to read (0 = none).
    pending: VecDeque<(Option<u64>, u64, i64)>,
    ticks: u64,     // Monotonic within this surface epoch; never reset by report().
    callbacks: u64, // Reporting window only.
    present_count: u64,
    last_present_ns: i64,
    gap_count: u64,
    gap_sum_us: u64,
    gap_max_us: u64,
    gap_over25: u64,
    gap_over35: u64,
    // Bounded histogram of actual display-present intervals: 1ms buckets.
    gap_bins: [u32; 51],
    pending_lost: u64,
    query_errors: u64,
    query_time_us: u64,
    // Button press to display present, for frames that read a new press.
    input_count: u64,
    input_sum_us: u64,
    input_max_us: u64,
    last_report: Instant,
}

impl PresentationProbe {
    pub(crate) fn new() -> Self {
        Self {
            pending: VecDeque::with_capacity(MAX_PENDING),
            ticks: 0,
            callbacks: 0,
            present_count: 0,
            last_present_ns: 0,
            gap_count: 0,
            gap_sum_us: 0,
            gap_max_us: 0,
            gap_over25: 0,
            gap_over35: 0,
            gap_bins: [0; 51],
            pending_lost: 0,
            query_errors: 0,
            query_time_us: 0,
            input_count: 0,
            input_sum_us: 0,
            input_max_us: 0,
            last_report: Instant::now(),
        }
    }
    pub(crate) fn tick(&mut self, source: &mut impl Source) {
        self.tick_with_input(source, 0);
    }
    /// `input_ns` is when the button press read by this callback happened.
    pub(crate) fn tick_with_input(&mut self, source: &mut impl Source, input_ns: i64) {
        self.ticks += 1;
        self.callbacks += 1;
        let started = Instant::now();
        // Read at most 2 pending timestamps per callback. A frame must have
        // passed through >=2 swaps before we query it, avoiding GPU waits.
        for _ in 0..2 {
            let Some(&(id, tick, pressed_ns)) = self.pending.front() else {
                break;
            };
            if self.ticks.saturating_sub(tick) < 3 {
                break;
            }
            let Some(id) = id else {
                self.pending.pop_front();
                // The capture error may have been reported in the previous
                // window. Account for the missing sample when its turn to
                // present arrives too, so this window cannot claim completeness.
                self.pending_lost += 1;
                self.last_present_ns = 0;
                continue;
            };
            let time = match source.timestamp(id) {
                Err(()) => {
                    self.query_errors += 1;
                    self.pending.pop_front();
                    self.last_present_ns = 0;
                    continue;
                }
                Ok(Timestamp::Pending) if self.ticks.saturating_sub(tick) <= 9 => break,
                Ok(Timestamp::Pending | Timestamp::Invalid) => {
                    self.pending_lost += 1;
                    self.pending.pop_front();
                    self.last_present_ns = 0;
                    continue;
                }
                Ok(Timestamp::Presented(time)) if time > 0 => time,
                Ok(Timestamp::Presented(_)) => {
                    self.pending_lost += 1;
                    self.pending.pop_front();
                    self.last_present_ns = 0;
                    continue;
                }
            };
            self.pending.pop_front();
            self.present_count += 1;
            if pressed_ns > 0 && time > pressed_ns && time - pressed_ns <= MAX_VALID_GAP_NS {
                let us = ((time - pressed_ns) / 1000) as u64;
                self.input_count += 1;
                self.input_sum_us += us;
                self.input_max_us = self.input_max_us.max(us);
            }
            if self.last_present_ns > 0 {
                let delta = time - self.last_present_ns;
                if delta > 0 && delta <= MAX_VALID_GAP_NS {
                    let us = (delta / 1000) as u64;
                    self.gap_count += 1;
                    self.gap_sum_us += us;
                    self.gap_max_us = self.gap_max_us.max(us);
                    if us > 25_000 {
                        self.gap_over25 += 1;
                    }
                    if us > 35_000 {
                        self.gap_over35 += 1;
                    }
                    let bucket = (us / 1000).min(50) as usize;
                    self.gap_bins[bucket] += 1;
                }
            }
            self.last_present_ns = time;
        }
        while self.pending.len() >= MAX_PENDING {
            self.pending.pop_front();
            self.pending_lost += 1;
            self.last_present_ns = 0;
        }
        match source.next_id() {
            Ok(id) => self.pending.push_back((Some(id), self.ticks, input_ns)),
            Err(()) => {
                self.query_errors += 1;
                // Preserve the discontinuity in queue order: an ID missed
                // now must break the gap *after* older pending IDs drain.
                self.pending.push_back((None, self.ticks, input_ns));
            }
        }
        self.query_time_us += started.elapsed().as_micros() as u64;
    }
    pub(crate) fn report(&mut self, status: &str) -> String {
        let p95 = if self.gap_count == 0 {
            0
        } else {
            let goal = self.gap_count.saturating_mul(95).div_ceil(100);
            let mut sum = 0u64;
            self.gap_bins
                .iter()
                .enumerate()
                .find_map(|(i, &n)| {
                    sum += u64::from(n);
                    (sum >= goal).then_some(i as u64 * 1000)
                })
                .unwrap_or(0)
        };
        let span = self.last_report.elapsed().as_secs_f64();
        // Do not publish an apparent presentation FPS when the driver has
        // dropped or rejected frame IDs. Missing samples must never be
        // converted into an apparent presentation FPS.
        let complete = status == "supported"
            && self.pending_lost == 0
            && self.query_errors == 0
            && self.gap_count >= 5;
        let verified_fps = if complete && self.gap_sum_us > 0 {
            self.gap_count as f64 * 1_000_000.0 / self.gap_sum_us as f64
        } else {
            -1.0
        };
        let status = if status == "supported" && !complete {
            "incomplete_timestamps"
        } else {
            status
        };
        let gap_mean = if complete && self.gap_count > 0 {
            (self.gap_sum_us / self.gap_count) as i64
        } else {
            -1
        };
        let gap_p95 = if complete { p95 as i64 } else { -1 };
        let gap_max = if complete { self.gap_max_us as i64 } else { -1 };
        let over25 = if complete { self.gap_over25 as i64 } else { -1 };
        let over35 = if complete { self.gap_over35 as i64 } else { -1 };
        let result = format!(
            "EGL presentation: status={} callbacks={} present_samples={} present_fps={:.2} \
             present_fps_valid={} report_span_s={:.3} \
             present_gap_mean_us={} present_gap_p95_us={} present_gap_max_us={} \
             present_over25={} present_over35={} lost={} errors={} \
             probe_us_per_callback={:.2} input_to_present_count={} \
             input_to_present_mean_us={} input_to_present_max_us={}",
            status,
            self.callbacks,
            self.present_count,
            verified_fps,
            complete,
            span,
            gap_mean,
            gap_p95,
            gap_max,
            over25,
            over35,
            self.pending_lost,
            self.query_errors,
            if self.callbacks > 0 {
                self.query_time_us as f64 / self.callbacks as f64
            } else {
                0.0
            },
            self.input_count,
            if self.input_count > 0 { (self.input_sum_us / self.input_count) as i64 } else { -1 },
            if self.input_count > 0 { self.input_max_us as i64 } else { -1 },
        );
        self.callbacks = 0;
        self.present_count = 0;
        self.gap_count = 0;
        self.gap_sum_us = 0;
        self.gap_max_us = 0;
        self.gap_over25 = 0;
        self.gap_over35 = 0;
        self.gap_bins = [0; 51];
        self.pending_lost = 0;
        self.query_errors = 0;
        self.query_time_us = 0;
        self.input_count = 0;
        self.input_sum_us = 0;
        self.input_max_us = 0;
        self.last_report = Instant::now();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Driver {
        next: u64,
        responses: HashMap<u64, Result<Timestamp, ()>>,
        queries: Vec<u64>,
        fail_next: bool,
    }
    impl Source for Driver {
        fn timestamp(&mut self, id: u64) -> Result<Timestamp, ()> {
            self.queries.push(id);
            self.responses
                .get(&id)
                .copied()
                .unwrap_or(Ok(Timestamp::Presented((id as i64 + 1) * 16_666_667)))
        }
        fn next_id(&mut self) -> Result<u64, ()> {
            let id = self.next;
            self.next += 1;
            if std::mem::take(&mut self.fail_next) {
                Err(())
            } else {
                Ok(id)
            }
        }
    }

    #[test]
    fn reports_do_not_rejuvenate_frames_waiting_for_presentation() {
        let mut probe = PresentationProbe::new();
        let mut driver = Driver::default();
        for window in 0..3 {
            for _ in 0..300 {
                probe.tick(&mut driver);
            }
            let report = probe.report("supported");
            assert!(report.contains("callbacks=300 "), "{report}");
            assert!(report.contains("present_fps_valid=true"), "{report}");
            assert!(report.contains("lost=0 errors=0"), "{report}");
            // The three frames awaiting feedback remain eligible in the next
            // report window. This fails if report() resets the age clock.
            assert_eq!(driver.queries.last(), Some(&(window * 300 + 296)));
        }
        assert_eq!(driver.queries, (0..897).collect::<Vec<_>>());
    }

    #[test]
    fn button_press_is_timed_to_the_present_of_the_frame_that_read_it() {
        let mut probe = PresentationProbe::new();
        let mut driver = Driver::default();
        for _ in 0..5 {
            probe.tick(&mut driver);
        }
        // Frame 5 presents at 6 * 16.67 ms; the press happened 40 ms earlier.
        probe.tick_with_input(&mut driver, 6 * 16_666_667 - 40_000_000);
        for _ in 0..5 {
            probe.tick(&mut driver);
        }
        let report = probe.report("supported");
        assert!(report.contains("input_to_present_count=1 "), "{report}");
        assert!(report.contains("input_to_present_mean_us=40000 "), "{report}");
        assert!(probe.report("supported").contains("input_to_present_mean_us=-1 "));
    }

    #[test]
    fn pending_query_is_deferred_without_waiting_and_budget_is_bounded() {
        let mut probe = PresentationProbe::new();
        let mut driver = Driver::default();
        driver.responses.insert(0, Ok(Timestamp::Pending));
        for _ in 0..7 {
            let before = driver.queries.len();
            probe.tick(&mut driver);
            assert!(driver.queries.len() - before <= 2);
        }
        assert_eq!(probe.pending_lost, 0);
        driver.responses.remove(&0);
        for _ in 0..10 {
            let before = driver.queries.len();
            probe.tick(&mut driver);
            assert!(driver.queries.len() - before <= 2);
        }
        assert_eq!(probe.pending_lost, 0);
        assert_eq!(probe.query_errors, 0);
    }

    #[test]
    fn expired_invalid_and_failed_timestamps_break_continuity() {
        for response in [Ok(Timestamp::Pending), Ok(Timestamp::Invalid), Err(())] {
            let mut probe = PresentationProbe::new();
            let mut driver = Driver::default();
            driver.responses.insert(4, response);
            for _ in 0..30 {
                probe.tick(&mut driver);
            }
            assert_eq!(probe.gap_over25, 0, "never count the hole as a slow frame");
            let report = probe.report("supported");
            assert!(report.contains("present_fps_valid=false"), "{report}");
            assert!(report.contains("present_fps=-1.00"), "{report}");
        }
    }

    #[test]
    fn missing_next_id_breaks_continuity_after_older_pending_frames() {
        let mut probe = PresentationProbe::new();
        let mut driver = Driver::default();
        for _ in 0..10 {
            probe.tick(&mut driver);
        }
        driver.fail_next = true;
        probe.tick(&mut driver);
        for _ in 0..10 {
            probe.tick(&mut driver);
        }
        assert_eq!(probe.gap_over25, 0);
        assert_eq!(probe.query_errors, 1);
        assert!(!driver.queries.contains(&10));
    }

    #[test]
    fn surface_reset_forgets_ids_and_timestamp_epoch() {
        let mut probe = PresentationProbe::new();
        let mut old = Driver::default();
        for _ in 0..100 {
            probe.tick(&mut old);
        }
        probe = PresentationProbe::new();
        let mut new = Driver::default();
        for _ in 0..20 {
            probe.tick(&mut new);
        }
        assert_eq!(new.queries, (0..17).collect::<Vec<_>>());
        assert_eq!(probe.gap_count, 16);
        assert_eq!(probe.gap_over25, 0);
    }

    #[test]
    fn next_id_error_before_report_stays_incomplete_when_the_missing_sample_matures() {
        let mut probe = PresentationProbe::new();
        let mut driver = Driver::default();
        for _ in 0..10 {
            probe.tick(&mut driver);
        }
        driver.fail_next = true;
        probe.tick(&mut driver);
        assert!(probe.report("supported").contains("errors=1"));
        for _ in 0..10 {
            probe.tick(&mut driver);
        }
        let report = probe.report("supported");
        assert!(report.contains("present_fps_valid=false"), "{report}");
        assert!(report.contains("lost=1 errors=0"), "{report}");
        assert_eq!(probe.gap_over25, 0);
    }

    #[test]
    fn incomplete_or_unsupported_timestamps_never_claim_presentation_fps() {
        let mut probe = PresentationProbe::new();
        assert!(probe
            .report("unsupported_extension")
            .contains("present_fps_valid=false"));
        let mut driver = Driver::default();
        for _ in 0..6 {
            probe.tick(&mut driver);
        }
        assert!(probe
            .report("supported")
            .contains("present_fps_valid=false"));
    }
}
