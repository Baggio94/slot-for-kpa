#!/usr/bin/env python3
"""Read Android SurfaceFlinger --timestats from an *offline* ADB text capture.

Uses real SurfaceFlinger presentToPresent histogram observations, unlike the
EGL frame-ID driver probe that may lose timestamp IDs. It is NOT photon latency
or a trace of controller input; a missing/unsupported layer is an inconclusive
measurement, never fabricated 59.7 FPS.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

LAYER_RE = re.compile(r"(?m)^\s*layerName\s*=\s*(.+?)\s*$")
TOTAL_RE = re.compile(r"\btotalFrames\s*=\s*(\d+)\b")
FPS_RE = re.compile(r"\baverageFPS\s*=\s*([0-9]+(?:\.[0-9]+)?)\b")
BUCKET_RE = re.compile(r"\b(\d+)ms\s*=\s*(\d+)\b")
PREFERRED_LAYER = "SurfaceView[fyi.slot.konkr/fyi.slot.konkr.MainActivity]"


def percentile(histogram: dict[int, int], percent: int) -> int | None:
    n = sum(histogram.values())
    if n <= 0:
        return None
    cutoff = (n * percent + 99) // 100
    seen = 0
    for milliseconds, count in sorted(histogram.items()):
        seen += count
        if seen >= cutoff:
            return milliseconds
    return None


def parse_timestats(raw: str) -> dict:
    """Select Slot's non-background SurfaceView and summarize present intervals."""
    sections = list(LAYER_RE.finditer(raw))
    candidates = []
    for i, match in enumerate(sections):
        label = match.group(1).strip()
        if PREFERRED_LAYER not in label or "Background" in label:
            continue
        chunk = raw[match.end():sections[i + 1].start()] if i + 1 < len(sections) else raw[match.end():]
        total = TOTAL_RE.search(chunk)
        fps = FPS_RE.search(chunk)
        # In the same layer block, isolate only present-to-present bins.
        # Android vendors use both "presentToPresent" (documented) and
        # "present2present" (KONKR Android 12) for the SAME histogram.
        # Parse only the contiguous bucket lines immediately after this header:
        # later latch2present/acquire2present histograms must never leak in.
        hist_title = re.search(
            r"(?:presentToPresent|present2present)\s+histogram\s+is\s+as\s+below\s*:",
            chunk, re.I,
        )
        bins = {}
        if hist_title:
            for line in chunk[hist_title.end():].splitlines():
                line = line.strip()
                if not line:
                    continue
                if not re.fullmatch(r"(?:\d+ms\s*=\s*\d+\s*)+", line):
                    break
                for bucket, count in BUCKET_RE.findall(line):
                    ms = int(bucket)
                    bins[ms] = bins.get(ms, 0) + int(count)
        candidates.append({
            "layer": label,
            "total_frames": int(total.group(1)) if total else None,
            "average_fps": float(fps.group(1)) if fps else None,
            "present_to_present_bins_ms": bins,
        })
    if not candidates:
        return {
            "status": "no_slot_surfaceview_timestats",
            "message": "No Slot SurfaceView presentToPresent layer. SurfaceFlinger command may be unsupported, denied or no game was visible.",
        }
    choice = max(candidates, key=lambda c: (c["total_frames"] or 0, sum(c["present_to_present_bins_ms"].values())))
    bins = choice.pop("present_to_present_bins_ms")
    total_observed = sum(bins.values())
    if total_observed <= 0:
        choice.update(status="no_present_histogram", observed_intervals=0)
        return choice
    # Intervals >25ms are notably longer than one 60Hz refresh, but this
    # threshold is not an Android jank classification or precise drop count.
    over25 = sum(count for ms, count in bins.items() if ms > 25)
    choice.update(
        status="ok", observed_intervals=total_observed,
        present_gap_p50_ms=percentile(bins, 50),
        present_gap_p95_ms=percentile(bins, 95),
        present_gap_p99_ms=percentile(bins, 99),
        present_gap_over25_count=over25,
        present_gap_over25_percent=round(100.0 * over25 / total_observed, 3),
    )
    return choice


def main() -> None:
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("file", type=Path, help="Raw adb dumpsys SurfaceFlinger --timestats -dump capture")
    cli.add_argument("--json", action="store_true", help="Machine-readable output")
    args = cli.parse_args()
    report = parse_timestats(args.file.read_text(encoding="utf-8", errors="replace"))
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    elif report["status"] != "ok":
        print(f"SurfaceFlinger: inconclusive ({report['status']})")
        print(report.get("message", report.get("layer", "")))
    else:
        print(f"Layer: {report['layer']}")
        print(f"SurfaceFlinger averageFPS: {report['average_fps']}")
        print(f"Frames: {report['total_frames']}; present intervals: {report['observed_intervals']}")
        print(f"Present gaps p50/p95/p99 ms: "
              f"{report['present_gap_p50_ms']}/{report['present_gap_p95_ms']}/{report['present_gap_p99_ms']}")
        print(f"Present gaps >25ms: {report['present_gap_over25_count']} "
              f"({report['present_gap_over25_percent']}%)")


if __name__ == "__main__":
    main()
