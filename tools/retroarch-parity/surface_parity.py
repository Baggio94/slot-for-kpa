#!/usr/bin/env python3
"""Parse actual SurfaceFlinger present intervals for slot or RetroArch.

Vendor 'averageFPS' is NOT emulator FPS; on the KONKR some firmware values
exceed its 60Hz panel. Uses only per-layer present2present/presentToPresent
histograms, never unrelated compositor global missedFrames/latency histograms.
No ROM, saves, framebuffer screenshots, input or audio data is read.
"""
from __future__ import annotations
import argparse
import json
import re
from pathlib import Path

LAYER = re.compile(r"(?m)^\s*layerName\s*=\s*(.+?)\s*$")
BUCKET = re.compile(r"(\d+)ms\s*=\s*(\d+)")
HIST = re.compile(r"(?:present2present|presentToPresent)\s+histogram\s+is\s+as\s+below\s*:", re.I)
FRAMES = re.compile(r"\btotalFrames\s*=\s*(\d+)")
DROPPED = re.compile(r"\bdroppedFrames\s*=\s*(\d+)")


def percentile(hist: dict[int, int], pct: int):
    total = sum(hist.values())
    if not total:
        return None
    need = (total * pct + 99) // 100
    seen = 0
    for ms, count in sorted(hist.items()):
        seen += count
        if seen >= need:
            return ms
    return None


def match_layer(name: str, target: str) -> bool:
    value = name.lower()
    if "background" in value:
        return False
    if target == "slot":
        return "fyi.slot.konkr" in value and "surfaceview" in value
    # RA may expose SurfaceView or Activity and package as layer label.
    return ("com.retroarch" in value and
            ("surfaceview" in value or "retroarch" in value))


def parse(raw: str, target: str) -> dict:
    if target not in ("ra", "slot"):
        raise ValueError("target must be ra or slot")
    blocks = list(LAYER.finditer(raw))
    options = []
    for i, header in enumerate(blocks):
        name = header.group(1)
        if not match_layer(name, target):
            continue
        chunk = raw[header.end():blocks[i + 1].start() if i + 1 < len(blocks) else len(raw)]
        m = HIST.search(chunk)
        if not m:
            continue
        buckets: dict[int, int] = {}
        for line in chunk[m.end():].splitlines():
            line = line.strip()
            if not line:
                continue
            if not re.fullmatch(r"(?:\d+ms\s*=\s*\d+\s*)+", line):
                break
            for ms, count in BUCKET.findall(line):
                b = int(ms)
                buckets[b] = buckets.get(b, 0) + int(count)
        if not buckets or sum(buckets.values()) == 0:
            continue
        frames = FRAMES.search(chunk)
        drops = DROPPED.search(chunk)
        options.append((sum(buckets.values()), {
            "target": target,
            "status": "ok",
            "total_frames": int(frames.group(1)) if frames else None,
            "dropped_frames": int(drops.group(1)) if drops else None,
            "observed_intervals": sum(buckets.values()),
            "p50_ms": percentile(buckets, 50),
            "p95_ms": percentile(buckets, 95),
            "p99_ms": percentile(buckets, 99),
            "over25_count": sum(c for ms, c in buckets.items() if ms > 25),
            "over25_fraction": round(sum(c for ms, c in buckets.items()
                                          if ms > 25) / sum(buckets.values()), 6),
            # No layerName in shared JSON: vendor names can leak other context.
            "notes": [
                "SurfaceFlinger per-layer present2present histogram, bucket resolution in ms.",
                "Not emulator frames, input-to-photon latency or proof of dropped frames.",
                "Compare RA/Slot using the same core/game/device/duration and settings."
            ],
        }))
    if not options:
        return {"target": target, "status": "inconclusive_no_matching_present_histogram",
                "observed_intervals": 0,
                "notes": ["No qualifying RetroArch/Slot layer histogram; do not infer zero jank."]}
    return max(options, key=lambda option: option[0])[1]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("raw", type=Path)
    p.add_argument("--target", choices=("ra", "slot"), required=True)
    p.add_argument("--output", type=Path)
    args = p.parse_args()
    data = parse(args.raw.read_text(encoding="utf-8", errors="replace"), args.target)
    content = json.dumps(data, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(content, encoding="utf-8")
        print(f"Sanitized presentation summary: {args.output} ({data['status']})")
    else:
        print(content, end="")


if __name__ == "__main__":
    main()
