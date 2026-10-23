#!/usr/bin/env python3
"""Noncausal SF A/B comparison regression tests."""
import unittest
from compare import compare


def sample(target, intervals=7200, slow=20):
    return {"target": target, "status": "ok", "observed_intervals": intervals,
            "over25_count": slow, "p99_ms": 21}


class ComparisonTests(unittest.TestCase):
    def test_normalized_percentages_are_by_intervals_not_raw_count(self):
        d = compare(sample("ra", 7200, 20), sample("slot", 7100, 30))
        self.assertEqual("descriptive_comparison_only", d["status"])
        self.assertEqual(20, d["ra"]["over25_count"])
        self.assertEqual(30, d["slot"]["over25_count"])
        self.assertGreater(d["slot_minus_ra_over25_percentage_points"], 0)

    def test_ra_missing_histogram_is_not_zero_jank(self):
        ra = {"target": "ra", "status": "inconclusive_no_matching_present_histogram"}
        d = compare(ra, sample("slot"))
        self.assertEqual("inconclusive_missing_capture", d["status"])

    def test_low_sample_capture_refuses_ranking(self):
        d = compare(sample("ra", 80, 0), sample("slot"))
        self.assertEqual("inconclusive_too_few_intervals", d["status"])

    def test_reverse_target_order_is_invalid(self):
        d = compare(sample("slot"), sample("ra"))
        self.assertEqual("invalid_target_labels", d["status"])


if __name__ == "__main__":
    unittest.main()
