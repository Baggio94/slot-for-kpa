#!/usr/bin/env python3
"""Regression tests: RetroArch and Slot vendor presentation statistics."""
import unittest
from surface_parity import match_layer, parse, percentile


class SurfaceParityTests(unittest.TestCase):
    RA = """
layerName = Background for SurfaceView[com.retroarch/com.retroarch.browser.retroactivity.RetroActivityFuture]
totalFrames = 40000
present2present histogram is as below:
16ms=40000
layerName = SurfaceView[com.retroarch/com.retroarch.browser.retroactivity.RetroActivityFuture](BLAST)#0
totalFrames = 7200
droppedFrames = 0
present2present histogram is as below:
0ms=0 16ms=6980 17ms=200
33ms=20
avg 16.1 max 33
latch2present histogram is as below:
70ms=70000
layerName = SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity](BLAST)#0
totalFrames = 7180
droppedFrames = 0
presentToPresent histogram is as below:
16ms=7150 33ms=30
averageFPS = 62.2
"""

    def test_retroarch_present_histogram_not_background_or_latency(self):
        report = parse(self.RA, "ra")
        self.assertEqual("ok", report["status"])
        self.assertEqual(7200, report["observed_intervals"])
        self.assertEqual(0, report["dropped_frames"])
        self.assertEqual(20, report["over25_count"])
        self.assertEqual(17, report["p99_ms"])
        self.assertEqual(16, report["p95_ms"])
        self.assertNotIn("layer_name", report)

    def test_slot_layer_independent_from_retroarch(self):
        report = parse(self.RA, "slot")
        self.assertEqual(7180, report["observed_intervals"])
        self.assertEqual(30, report["over25_count"])
        self.assertEqual(16, report["p99_ms"])

    def test_unknown_or_denied_is_explicitly_inconclusive(self):
        for text in ("", "Permission Denial", "Unknown option"):
            self.assertEqual(
                "inconclusive_no_matching_present_histogram",
                parse(text, "ra")["status"])

    def test_not_a_garbage_collector_of_other_histograms(self):
        raw = """
layerName = SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity](BLAST)#5
totalFrames = 5
present2present histogram is as below:
16ms=4
33ms=1
some other field = 400
latch2present histogram is as below:
99ms=500
"""
        report = parse(raw, "slot")
        self.assertEqual(5, report["observed_intervals"])
        self.assertEqual(1, report["over25_count"])

    def test_layer_matching_refuses_background(self):
        self.assertFalse(match_layer("Background for SurfaceView[com.retroarch]", "ra"))
        self.assertFalse(match_layer("com.other.Activity", "slot"))

    def test_empty_percentile_and_threshold(self):
        self.assertIsNone(percentile({}, 99))
        self.assertEqual(33, percentile({16: 99, 33: 1}, 100))


if __name__ == "__main__":
    unittest.main()
