#!/usr/bin/env python3
"""Offline ADB log analysis for Slot Emulation Lab. No ROM/audio content collected.

Usage: python3 tools/emulation-lab/report.py captured.log
The 5s windows report callback pacing, audio underruns and speculative work.
These data do not measure input-to-photon latency or objectively grade audio.
"""
from __future__ import annotations
import argparse
import json
import re
from pathlib import Path
from statistics import mean, median

TAG = "SlotEmuDiag"
KEYVAL = re.compile(r"([a-z][a-z0-9_]+)=([^\s]+)")

def parse_log(source: str, expected_duration_s: int | None = None) -> dict:
    samples = []
    inputs = []
    other = []
    for line in source.splitlines():
        if TAG in line and "SAMPLE " in line:
            data = dict(KEYVAL.findall(line.split("SAMPLE ", 1)[-1]))
            samples.append(data)
        elif TAG in line and "INPUT " in line:
            inputs.append(dict(KEYVAL.findall(line.split("INPUT ", 1)[-1])))
        elif ("A12 PCM pacing:" in line or "Run-Ahead disabled:" in line
              or "GL callback pacing:" in line):
            other.append(line.split(": ", 1)[-1].strip())
    def values(key: str) -> list[float]:
        acc = []
        for s in samples:
            try:
                acc.append(float(s[key]))
            except (KeyError, ValueError, TypeError):
                pass
        return acc
    def measurement(key: str):
        data = values(key)
        return {"n": len(data), "mean": round(mean(data), 3),
                "median": round(median(data), 3),
                "min": round(min(data), 3), "max": round(max(data), 3)} if data else None
    underflows = values("audiotrack_underruns")
    observed_s = sum(values("span_ms")) / 1000.0
    cores = sorted({s["core"] for s in samples if "core" in s})
    runahead = sorted({s["runahead_requested"] for s in samples
                       if "runahead_requested" in s})
    pacing_modes = sorted({s["pacing_mode"] for s in samples
                           if "pacing_mode" in s})
    # Consecutive windows that share game/core/timing configuration.
    # Prevent a previous game's first 5s window from being quietly
    # averaged with a newly selected Run-Ahead or core.
    segments: list[dict] = []
    for sample in samples:
        config = {
            "core": sample.get("core", "unknown"),
            "runahead_requested": sample.get("runahead_requested", "unknown"),
            "pacing_mode": sample.get("pacing_mode", "unknown"),
        }
        if not segments or segments[-1]["config"] != config:
            segments.append({"config": config, "windows": 0, "duration_s": 0.0,
                             "_emu_fps": []})
        segment = segments[-1]
        segment["windows"] += 1
        try:
            segment["duration_s"] += float(sample.get("span_ms", 0)) / 1000.0
        except (ValueError, TypeError):
            pass
        try:
            segment["_emu_fps"].append(float(sample["emu_fps"]))
        except (KeyError, ValueError, TypeError):
            pass
    for segment in segments:
        segment["duration_s"] = round(segment["duration_s"], 2)
        fps = segment.pop("_emu_fps")
        segment["emu_fps_mean"] = round(mean(fps), 3) if fps else None

    warnings = [
        "GL callback FPS is not display presentation FPS (EGL swap occurs later)",
        "GL gap quantiles are approximate (1ms buckets) and are not presentation times",
        "Emulation step0/1/multi counters are one-second snapshots, not five-second totals",
        "Input-to-next-GL callback is not physical input-to-photon latency",
        "PCM jump candidates can be legitimate game audio transients",
        "Mac AVD results do not establish KONKR hardware performance",
        "Compare only same game/scenario, emulator core, settings and device",
    ]
    if len(segments) > 1:
        warnings.insert(0,
            "MIXED CAPTURE: core, pacing or Run-Ahead changed while recording; "
            "whole-capture averages combine settings. Review capture_segments.")
    if expected_duration_s and observed_s < 0.8 * expected_duration_s:
        warnings.insert(0,
            f"PARTIAL CAPTURE: only {observed_s:.2f}s of {expected_duration_s}s "
            "requested contains game samples. Start game before recording.")

    return {
        "windows": len(samples),
        "duration_s": round(observed_s, 2),
        "requested_duration_s": expected_duration_s,
        "capture_segments": segments,
        "cores_observed": cores,
        "requested_runahead": runahead,
        "pacing_modes_observed": pacing_modes,
        "emu_fps": measurement("emu_fps"),
        "target_fps": measurement("target_fps"),
        "gl_callback_fps": measurement("gl_callback_fps"),
        "gl_gap_p50_us": measurement("gl_gap_p50_us"),
        "gl_gap_p95_us": measurement("gl_gap_p95_us"),
        "gl_gap_p99_us": measurement("gl_gap_p99_us"),
        "emu_step0_1s": measurement("emu_step0_1s"),
        "emu_step1_1s": measurement("emu_step1_1s"),
        "emu_step_multi_1s": measurement("emu_step_multi_1s"),
        "display_callback_fps": measurement("display_callbacks_fps"),
        "gl_over25_total": round(sum(values("gl_over25"))),
        "gl_over40_total": round(sum(values("gl_over40"))),
        "runahead_calls_total": round(sum(values("runahead_calls"))),
        "runahead_failures_total": round(sum(values("runahead_failures"))),
        "runahead_max_us": measurement("runahead_max_us"),
        "runahead_snapshot_us_total": round(sum(values("runahead_snapshot_us"))),
        "runahead_speculative_us_total": round(sum(values("runahead_speculative_us"))),
        "runahead_restore_us_total": round(sum(values("runahead_restore_us"))),
        "input_down_total": round(sum(values("input_down"))),
        "input_next_gl_mean_us": measurement("input_next_gl_mean_us"),
        "input_next_gl_max_us": measurement("input_next_gl_max_us"),
        "audiotrack_underruns_delta": round(
            sum(max(0, b - a) for a, b in zip(underflows, underflows[1:])))
            if underflows else None,
        "audio_empty_total": round(sum(values("audio_empty"))),
        "audio_jump_candidates_total": round(sum(values("audio_jump_candidates"))),
        "audio_queued_ms": measurement("audio_queued_ms"),
        "input_events": len(inputs),
        "other_diagnostics": other[-50:],
        "warnings": warnings,
    }

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=Path)
    parser.add_argument("--json", action="store_true", help="output JSON")
    parser.add_argument("--expected-seconds", type=int, default=None,
                        help="requested wall-clock capture duration")
    args = parser.parse_args()
    result = parse_log(args.log.read_text(errors="replace"),
                       expected_duration_s=args.expected_seconds)
    if args.json:
        print(json.dumps(result, indent=2, ensure_ascii=False))
        return
    print(f"Slot Emulation Lab — {result['windows']} windows, {result['duration_s']}s")
    if not result["windows"]:
        print("No SAMPLE measurements detected: launch a game and record at least 10s.")
        return
    print("Core:", ", ".join(result["cores_observed"]), "| Run-Ahead:",
          ", ".join(result["requested_runahead"]))
    if result["pacing_modes_observed"]:
        print("Pacing:", ", ".join(result["pacing_modes_observed"]))
    if len(result["capture_segments"]) > 1:
        print("Capture contains multiple game configurations:")
        for idx, segment in enumerate(result["capture_segments"], 1):
            config = segment["config"]
            print(f"  {idx}: {config['core']} RA={config['runahead_requested']} "
                  f"pacing={config['pacing_mode']} | "
                  f"{segment['windows']} windows, {segment['duration_s']}s, "
                  f"sampled FPS={segment['emu_fps_mean']}")
    for key, label in (
        ("emu_fps", "Core emulated FPS"),
        ("gl_callback_fps", "GL callback FPS"),
        ("gl_gap_p50_us", "GL gap P50 (us)"),
        ("gl_gap_p95_us", "GL gap P95 (us)"),
        ("gl_gap_p99_us", "GL gap P99 (us)"),
        ("emu_step0_1s", "Zero-step callbacks / 1s"),
        ("emu_step_multi_1s", "Multi-step callbacks / 1s"),
        ("input_next_gl_mean_us", "Input-to-next GL callback (us)"),
        ("runahead_max_us", "Slowest Run-Ahead call (us)"),
        ("audio_queued_ms", "Native queued audio (ms)")):
        v = result[key]
        if v:
            print(f"{label:34} mean={v['mean']:.2f} "
                  f"median={v['median']:.2f} min={v['min']:.2f} max={v['max']:.2f}")
    for key in (
        "gl_over25_total", "gl_over40_total", "runahead_calls_total",
        "runahead_failures_total", "audiotrack_underruns_delta",
        "audio_empty_total", "audio_jump_candidates_total", "input_down_total",
    ):
        print(f"{key:34} {result[key]}")
    print("\nInterpretation limits:")
    for warning in result["warnings"]:
        print(" -", warning)

if __name__ == "__main__":
    main()
