#!/usr/bin/env python3
"""No-device tests for RetroArch config extraction; offline and privacy-focused."""
from pathlib import Path
import json
import tempfile
import unittest

from parse_ra_config import parse_cfg, summarize


class ConfigExtractionTests(unittest.TestCase):
    def test_actual_runahead_and_android_av_keys(self):
        data = parse_cfg('''
            # video_vsync = false
            run_ahead_enabled = "true"
            run_ahead_frames = "1"
            run_ahead_secondary_instance = "true"
            video_vsync = "true"
            video_max_swapchain_images = "2"
            audio_latency = "64"
            audio_sync = "true"
            input_poll_type_behavior = "1"
        ''')
        self.assertEqual("true", data["run_ahead_enabled"])
        self.assertEqual("1", data["run_ahead_frames"])
        self.assertEqual("true", data["run_ahead_secondary_instance"])
        self.assertEqual("2", data["video_max_swapchain_images"])

    def test_private_paths_credentials_and_network_are_never_emitted(self):
        data = parse_cfg('''
            libretro_path = "/storage/0/private/secret.so"
            cheevos_token = "token-123"
            username = "Sensitive"
            video_vsync = "true"
            mgba_frameskip = "disabled"
            gpsp_frameskip = "auto"
        ''')
        self.assertEqual({"video_vsync": "true",
                          "mgba_frameskip": "disabled",
                          "gpsp_frameskip": "auto"}, data)

    def test_whitelist_rejects_values_with_paths_and_metacharacters(self):
        data = parse_cfg('''
            video_driver = "/storage/some/driver"
            audio_driver = "aa|payload"
            video_vsync = "{bad}"
            mgba_frameskip = "disabled"
        ''')
        self.assertNotIn("video_driver", data)
        self.assertNotIn("audio_driver", data)
        self.assertNotIn("video_vsync", data)
        self.assertEqual("disabled", data["mgba_frameskip"])

    def test_frame_pacing_controls_are_verified_safe_scalar_keys(self):
        data = parse_cfg('''
            video_threaded = "false"
            video_threaded_present_repeat = "true"
            video_threaded_display_pacing = "true"
            video_scanline_sync = "false"
            video_scanline_sync_offset = "0"
            vrr_runloop_enable = "false"
            video_frame_delay_auto = "true"
            audio_rate_control = "true"
            audio_rate_control_delta = "0.005"
            video_swap_interval = "1"
        ''')
        self.assertEqual("true", data["video_threaded_display_pacing"])
        self.assertEqual("false", data["vrr_runloop_enable"])
        self.assertEqual("0.005", data["audio_rate_control_delta"])
        self.assertEqual("1", data["video_swap_interval"])

    def test_missing_configuration_is_explicitly_unknown(self):
        result = summarize(None, None, "mgba")
        self.assertEqual({}, result["settings"])
        self.assertIn("video_vsync", result["unknown_settings"])
        self.assertIn("mgba_frameskip", result["unknown_settings"])
        self.assertEqual([], result["sources_provided"])

    def test_mgba_report_never_contains_gpsp_values(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root, "retroarch.cfg")
            path.write_text('''gpsp_frameskip = "auto"
mgba_frameskip = "disabled"
video_vsync = "true"
''')
            result = summarize(str(path), None, "mgba")
            self.assertEqual(result["settings"]["mgba_frameskip"], "disabled")
            self.assertNotIn("gpsp_frameskip", json.dumps(result))
            self.assertEqual(["retroarch_cfg"], result["sources_provided"])

    def test_core_options_override_config_when_supplied(self):
        with tempfile.TemporaryDirectory() as root:
            ra, opt = Path(root, "retroarch.cfg"), Path(root, "options.opt")
            ra.write_text('''mgba_frameskip = "auto"
video_vsync = "true"''')
            opt.write_text('''mgba_frameskip = "disabled"''')
            data = summarize(str(ra), str(opt), "mgba")
            self.assertEqual("disabled", data["settings"]["mgba_frameskip"])
            self.assertEqual(["retroarch_cfg", "core_options"], data["sources_provided"])

    def test_invalid_or_unclosed_quote_cannot_leak(self):
        data = parse_cfg('''run_ahead_frames = "2
video_vsync = true
video_refresh_rate = "59.728"
''')
        self.assertNotIn("run_ahead_frames", data)
        self.assertEqual("true", data["video_vsync"])
        self.assertEqual("59.728", data["video_refresh_rate"])

    def test_does_not_emit_unknown_options_even_if_they_look_like_core_options(self):
        data = parse_cfg('''mgba_user_path = "sensitive"
mgba_frameskip_type = "disabled"
mgba_cheevos_username = "sensitive"
''')
        self.assertEqual({"mgba_frameskip_type": "disabled"}, data)


if __name__ == "__main__":
    unittest.main()
