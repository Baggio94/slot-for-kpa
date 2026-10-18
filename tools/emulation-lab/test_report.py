"""Regression tests for the offline, read-only performance log parser."""
import unittest
from report import parse_log

class EmulationLabReportTests(unittest.TestCase):
    def test_two_mgba_runahead_samples(self):
        sample = ("I/SlotEmuDiag: SAMPLE span_ms=5000 gl_callback_fps=59.800 "
                  "gl_over25=1 gl_over40=0 input_down=4 input_next_gl_count=4 "
                  "input_next_gl_mean_us=4310 input_next_gl_max_us=8890 "
                  "audio_chunks=900 audio_pcm_shorts=420000 audio_empty=3 "
                  "audio_jump_candidates=0 audio_queued_ms=25.0 audiotrack_underruns=3 "
                  "core=mgba emu_fps=59.727 target_fps=59.728 "
                  "display_callbacks_fps=59.800 catchup_percent=0.0 "
                  "runahead_requested=1 runahead_calls=299 "
                  "runahead_snapshot_us=13000 runahead_speculative_us=25000 "
                  "runahead_restore_us=10000 runahead_max_us=400 "
                  "runahead_failures=0 linked=false")
        second = sample.replace("audiotrack_underruns=3", "audiotrack_underruns=5") \
                       .replace("gl_over25=1", "gl_over25=2")
        result = parse_log(sample + "\n" + second)
        self.assertEqual(result["windows"], 2)
        self.assertEqual(result["duration_s"], 10.0)
        self.assertEqual(result["cores_observed"], ["mgba"])
        self.assertEqual(result["requested_runahead"], ["1"])
        self.assertAlmostEqual(result["emu_fps"]["mean"], 59.727)
        self.assertEqual(result["runahead_calls_total"], 598)
        self.assertEqual(result["runahead_failures_total"], 0)
        self.assertEqual(result["audiotrack_underruns_delta"], 2)
        self.assertEqual(result["gl_over25_total"], 3)

    def test_ignores_other_logs_and_tolerates_missing_fields(self):
        text = ("I/SlotKonkr: A12 PCM pacing: rate=44100\n"
                "I/SlotEmuDiag: INPUT event=down key=KEYCODE_ENTER\n"
                "I/SlotEmuDiag: SAMPLE span_ms=5010 core=gpsp emu_fps=59.71\n"
                "W/Other: SAMPLE span_ms=5000 core=mgba emu_fps=5\n")
        r = parse_log(text)
        self.assertEqual(r["windows"], 1)
        self.assertEqual(r["cores_observed"], ["gpsp"])
        self.assertEqual(r["input_events"], 1)
        self.assertIsNone(r["audio_queued_ms"])
        self.assertAlmostEqual(r["emu_fps"]["mean"], 59.71)

    def test_empty_capture_has_no_fake_success(self):
        r = parse_log("Device started, no game running")
        self.assertEqual(r["windows"], 0)
        self.assertIsNone(r["emu_fps"])
        self.assertEqual(r["duration_s"], 0)
        self.assertEqual(r["input_events"], 0)

if __name__ == "__main__":
    unittest.main()
