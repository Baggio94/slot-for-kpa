#!/usr/bin/env python3
"""CI tests for the read-only KONKR SurfaceFlinger present histogram parser."""
import unittest

from surfaceflinger_timestats import parse_timestats, percentile


class SurfaceFlingerStatsTests(unittest.TestCase):
    def test_real_slot_surfaceview_histogram_and_percentiles(self):
        raw = """
layerName = SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity]@0(BLAST)#23
totalFrames = 300
presentToPresent histogram is as below:
0ms=0 15ms=1 16ms=280 17ms=5 33ms=12 50ms=2
averageFPS = 59.724
"""
        report = parse_timestats(raw)
        self.assertEqual("ok", report["status"])
        self.assertEqual(300, report["total_frames"])
        self.assertEqual(300, report["observed_intervals"])
        self.assertAlmostEqual(59.724, report["average_fps"])
        self.assertEqual(16, report["present_gap_p50_ms"])
        self.assertEqual(16, report["present_gap_p95_ms"])
        self.assertEqual(33, report["present_gap_p99_ms"])
        self.assertEqual(14, report["present_gap_over25_count"])

    def test_only_main_surfaceview_not_background_or_unrelated_layer(self):
        raw = """
layerName = Background for SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity]#0
totalFrames = 2000
presentToPresent histogram is as below:
16ms=2000
averageFPS = 60
layerName = SurfaceView[com.other/.Activity]
totalFrames = 3000
presentToPresent histogram is as below:
16ms=3000
averageFPS = 60
layerName = SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity](BLAST)#4
totalFrames = 55
presentToPresent histogram is as below:
16ms=54
averageFPS = 59.7
"""
        report = parse_timestats(raw)
        self.assertEqual("ok", report["status"])
        self.assertEqual(55, report["total_frames"])
        self.assertEqual(54, report["observed_intervals"])

    def test_unsupported_dump_does_not_invent_valid_present_fps(self):
        for raw in ("Permission Denial: can't dump SurfaceFlinger", "Unknown option", ""):
            self.assertEqual("no_slot_surfaceview_timestats", parse_timestats(raw)["status"])

    def test_missing_present_histogram_is_inconclusive(self):
        raw = """
layerName = SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity](BLAST)#0
totalFrames = 400
averageFPS = 59.8
"""
        report = parse_timestats(raw)
        self.assertEqual("no_present_histogram", report["status"])
        self.assertEqual(0, report["observed_intervals"])

    def test_multiple_histograms_choose_only_actual_present_intervals(self):
        raw = """
layerName = SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity](BLAST)#0
totalFrames = 4
presentToPresent histogram is as below:
16ms=3 33ms=1
presentToAcquire histogram is as below:
50ms=999
averageFPS = 59.7
"""
        report = parse_timestats(raw)
        self.assertEqual("ok", report["status"])
        self.assertEqual(4, report["observed_intervals"])
        self.assertEqual(1, report["present_gap_over25_count"])
        self.assertEqual(25.0, report["present_gap_over25_percent"])

    def test_empty_histogram_percentile_is_not_measured(self):
        self.assertIsNone(percentile({}, 95))
        self.assertEqual(33, percentile({16: 4, 33: 1}, 95))


if __name__ == "__main__":
    unittest.main()
