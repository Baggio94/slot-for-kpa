#!/usr/bin/env python3
"""Extract ONLY allowlisted RetroArch GBA performance settings from a local cfg.

No ROM paths, save paths, usernames, server addresses, credentials or other
configuration values ever reach the report. Missing settings are UNKNOWN, not
assumed to be RetroArch defaults. Supports RetroArch .cfg and core .opt files.
This is a reference/measurement tool, NOT a RetroArch runtime replacement.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

RA_KEYS = (
    "run_ahead_enabled", "run_ahead_frames", "run_ahead_secondary_instance",
    "runahead_enabled", "runahead_frames", "runahead_secondary_instance",
    "preempt_frames_enable", "preempt_frames",
    "video_driver", "video_vsync", "video_swap_interval",
    "video_max_swapchain_images", "video_hard_sync", "video_hard_sync_frames",
    "video_frame_delay", "video_frame_delay_auto", "video_threaded",
    "video_refresh_rate", "video_refresh_rate_auto", "video_smooth",
    "video_shader_enable", "video_filter", "video_black_frame_insertion",
    "audio_driver", "audio_sync", "audio_latency", "audio_rate_control",
    "audio_rate_control_delta", "audio_max_timing_skew", "audio_out_rate",
    "input_driver", "input_poll_type_behavior", "rewind_enable",
    "fastforward_ratio", "video_frame_delay_auto"
)
MGBA_KEYS = (
    "mgba_frameskip", "mgba_frameskip_threshold", "mgba_frameskip_interval",
    "mgba_use_bios", "mgba_skip_bios", "mgba_color_correction",
    "mgba_interframe_blending", "mgba_audio_low_pass_filter",
    "mgba_audio_low_pass_range", "mgba_gb_colors_preset",
    "mgba_game_boy_player_rumble", "mgba_allow_opposing_directions",
    "mgba_frameskip_type", "mgba_force_gbp"
)
GPSP_KEYS = (
    "gpsp_frameskip", "gpsp_frameskip_threshold", "gpsp_frameskip_interval",
    "gpsp_bios", "gpsp_boot_mode", "gpsp_color_correction",
    "gpsp_interframe_blending", "gpsp_drc", "gpsp_drc_enable",
    "gpsp_frameskip_type", "gpsp_use_bios"
)
KEYS = tuple(dict.fromkeys(RA_KEYS + MGBA_KEYS + GPSP_KEYS))
ALLOW = frozenset(KEYS)
# Only basic scalar settings accepted, never paths or potentially private text.
SAFE_VALUE = frozenset("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ._+-:%")


def parse_cfg(text: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith(("#", ";")) or "=" not in line:
            continue
        key, value = (part.strip() for part in line.split("=", 1))
        if key not in ALLOW:
            continue
        if value.startswith('"'):
            if not value.endswith('"') or len(value) < 2:
                continue
            value = value[1:-1]
        if not value or len(value) > 80 or not set(value) <= SAFE_VALUE:
            continue
        result[key] = value
    return result


def summarize(config: str | None, options: str | None, core: str) -> dict:
    found: dict[str, str] = {}
    provided = []
    for label, source in (("retroarch_cfg", config), ("core_options", options)):
        if source is None:
            continue
        found.update(parse_cfg(Path(source).read_text(encoding="utf-8", errors="replace")))
        provided.append(label)
    relevant = tuple(key for key in KEYS
                     if key in RA_KEYS or key.startswith(core + "_"))
    present = {k: found[k] for k in relevant if k in found}
    # An absent field may be an implicit default: never assert false/0.
    return {
        "schema": "slot-retroarch-parity-v1",
        "core": core, "sources_provided": provided,
        "settings": present,
        "unknown_settings": [k for k in relevant if k not in found],
        "notes": [
            "Absent means unknown, not RetroArch default.",
            "This report contains only allowlisted scalar settings; no paths or secrets.",
            "Settings describe configuration, not runtime core build version.",
            "Run-Ahead secondary core and Preemptive Frames require explicit verification.",
            "Core-options overrides can be content-directory or per-game specific."
        ]
    }


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--retroarch-cfg", help="Local RetroArch .cfg")
    p.add_argument("--core-options", help="Local mGBA/gpSP .opt")
    p.add_argument("--core", choices=("mgba", "gpsp"), required=True)
    p.add_argument("--output", type=Path)
    args = p.parse_args()
    result = summarize(args.retroarch_cfg, args.core_options, args.core)
    payload = json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    if args.output:
        args.output.write_text(payload, encoding="utf-8")
        print(f"Safe, allowlisted report saved to {args.output}")
    else:
        print(payload, end="")


if __name__ == "__main__":
    main()
