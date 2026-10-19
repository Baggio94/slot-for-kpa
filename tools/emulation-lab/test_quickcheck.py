"""Read-only quick-check regression gates. Runs in existing production CI."""

import unittest

from quickcheck import markdown, triage


def sample(core="gpsp", ra="1", unders="0", mode="sync_lab"):
    return ("I/SlotEmuDiag: SAMPLE span_ms=5000 "
            f"core={core} runahead_requested={ra} pacing_mode={mode} "
            "emu_fps=59.728 gl_callback_fps=59.91 "
            f"runahead_failures=0 audiotrack_underruns={unders}")


def log(headrooms=(40.0, 35.0), core="gpsp", ra="1", fail=None,
        early=True):
    text = "\n".join([sample(core=core, ra=ra)] * 10)
    for n in headrooms:
        text += f"\nI/SlotKonkr: AudioTrack queue depth: sample_count=300 minimum_ms={n} maximum_ms=80.0"
    if early:
        text += "\nI/SlotKonkr: GL callback pacing: early_pcm_wakes_5s=299"
    if fail:
        text += f"\nI/SlotEmuDiag: SAMPLE span_ms=5000 core={core} runahead_requested={ra} pacing_mode=sync_lab runahead_failures={fail}"
    return text


def report(unders=0, failure=0, duration=50):
    return {
        "duration_s": duration, "requested_duration_s": 50,
        "audiotrack_underruns_delta": unders,
        "runahead_failures_total": failure,
        "emu_fps": {"mean": 59.728},
        "gl_callback_fps": {"mean": 59.91},
        "gl_over25_total": 1,
    }


class QuickCheckTests(unittest.TestCase):
    def test_healthy_capture_still_only_provisional(self):
        result = triage(report(), log(), "gpsp", 1)
        self.assertEqual(result["status"], "OK PROVISOIRE")
        self.assertEqual(result["underruns"], 0)
        self.assertEqual(result["early_pcm_wakes"], 299)
        self.assertIn("provisoire", markdown(result).lower())

    def test_near_empty_audio_output_is_not_success(self):
        result = triage(report(), log(headrooms=(20, 0.061)), "gpsp", 1)
        self.assertEqual(result["status"], "SURVEILLER")
        self.assertEqual(result["output_windows_below_1ms"], 1)
        self.assertTrue(any("Réserve AudioTrack" in c for c in result["cautions"]))

    def test_hardware_underrun_is_blocking(self):
        result = triage(report(unders=27), log(), "gpsp", 1)
        self.assertEqual(result["status"], "A REVOIR")
        self.assertTrue(any("27 underruns" in x for x in result["issues"]))

    def test_missing_game_or_mixed_config_fails(self):
        self.assertEqual(triage(report(), "")["status"], "A REVOIR")
        mixed = log() + "\n" + sample(core="mgba")
        result = triage(report(), mixed)
        self.assertEqual(result["status"], "A REVOIR")
        self.assertTrue(any("Réglages modifiés" in x for x in result["issues"]))

    def test_expected_core_and_runahead_guard(self):
        result = triage(report(), log(), expected_core="mgba", expected_ra=0)
        self.assertEqual(result["status"], "A REVOIR")
        self.assertEqual(len(result["issues"]), 2)

    def test_partially_recorded_run_fails(self):
        result = triage(report(duration=20), log())
        self.assertEqual(result["status"], "A REVOIR")
        self.assertTrue(any("partielle" in x for x in result["issues"]))

    def test_no_output_queue_measurement_is_uncertain(self):
        result = triage(report(), "\n".join([sample()] * 10))
        self.assertEqual(result["status"], "SURVEILLER")

    def test_egl_errors_cannot_be_called_present_fps(self):
        data = log()
        data += "\nI/SlotKonkr: EGL presentation: status=supported present_fps=57.66 lost=5 errors=7"
        result = triage(report(), data)
        self.assertEqual(result["status"], "OK PROVISOIRE")
        self.assertTrue(any("EGL" in x for x in result["cautions"]))

    def test_runahead_failures_are_blocking(self):
        result = triage(report(failure=2), log())
        self.assertEqual(result["status"], "A REVOIR")


if __name__ == "__main__":
    unittest.main()
