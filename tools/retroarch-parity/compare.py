#!/usr/bin/env python3
"""Compare matched RetroArch/slot present timing from sanitized JSON reports.

A single A/B pair is descriptive only. Do not conclude superiority, true
emulator FPS, latency, dropped frames or statistical significance.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path


def compare(ra: dict, slot: dict) -> dict:
    if ra.get("status") != "ok" or slot.get("status") != "ok":
        return {
            "status": "inconclusive_missing_capture",
            "ra_status": ra.get("status", "missing"),
            "slot_status": slot.get("status", "missing"),
        }
    if ra.get("target") != "ra" or slot.get("target") != "slot":
        return {"status": "invalid_target_labels"}
    nra, nslot = ra.get("observed_intervals", 0), slot.get("observed_intervals", 0)
    if nra < 1000 or nslot < 1000:
        return {"status": "inconclusive_too_few_intervals",
                "ra_intervals": nra, "slot_intervals": nslot}
    ra_pct = 100 * ra["over25_count"] / nra
    slot_pct = 100 * slot["over25_count"] / nslot
    return {
        "status": "descriptive_comparison_only",
        "ra": {"intervals": nra, "over25_count": ra["over25_count"],
               "over25_percent": round(ra_pct, 4), "p99_ms": ra["p99_ms"]},
        "slot": {"intervals": nslot, "over25_count": slot["over25_count"],
                 "over25_percent": round(slot_pct, 4), "p99_ms": slot["p99_ms"]},
        "slot_minus_ra_over25_percentage_points": round(slot_pct - ra_pct, 4),
        "notes": [
            "ONE A/B sample does not establish causal or statistical differences.",
            "Repeat at least A/B/A on same ROM/core/settings/session length and thermal state.",
            "SurfaceFlinger intervals are NOT emulation FPS or button-to-photon latency.",
            "No assumptions about unknown RetroArch configuration or core version.",
            "Compare subjective audio/input and Run-Ahead separately."
        ],
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("ra_summary", type=Path)
    p.add_argument("slot_summary", type=Path)
    p.add_argument("--output", type=Path)
    args = p.parse_args()
    ra = json.loads(args.ra_summary.read_text(encoding="utf-8"))
    slot = json.loads(args.slot_summary.read_text(encoding="utf-8"))
    result = compare(ra, slot)
    raw = json.dumps(result, indent=2, ensure_ascii=False) + "\n"
    if args.output:
        args.output.write_text(raw, encoding="utf-8")
        print(f"Comparison saved: {args.output} ({result['status']})")
    else:
        print(raw, end="")


if __name__ == "__main__":
    main()
