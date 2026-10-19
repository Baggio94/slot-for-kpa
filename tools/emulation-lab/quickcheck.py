#!/usr/bin/env python3
"""slot. quick check: one-command safe ADB capture + offline audio/video triage.

Examples (from the beta3 test-kit root, game already running):
    python3 quickcheck.py --serial BW0308N250009576 --seconds 180
    python3 quickcheck.py --existing emulation-lab-results/2026...-run/
No settings, emulator cores, input, ROMs, saves or app data are changed.
Requires Python 3.9+; only Python stdlib plus existing capture.sh/report.py.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from statistics import median

PAIR = re.compile(r"([a-z][a-z0-9_]+)=([^\s]+)")
SAFE = re.compile(r"^[A-Za-z0-9._:-]+$")


def values(line: str) -> dict[str, str]:
    return dict(PAIR.findall(line))


def triage(report: dict, logcat: str, expected_core: str | None = None,
           expected_ra: int | None = None) -> dict:
    samples = [values(s.partition("SAMPLE ")[2]) for s in logcat.splitlines()
               if "SlotEmuDiag" in s and "SAMPLE " in s]
    depths = []
    early = []
    egl = []
    for line in logcat.splitlines():
        if "AudioTrack queue depth:" in line:
            m = values(line.partition("AudioTrack queue depth:")[2])
            try:
                depths.append(float(m["minimum_ms"]))
            except (KeyError, ValueError):
                pass
        if "GL callback pacing:" in line:
            m = values(line.partition("GL callback pacing:")[2])
            if "early_pcm_wakes_5s" in m:
                try:
                    early.append(int(m["early_pcm_wakes_5s"]))
                except ValueError:
                    pass
        if "EGL presentation:" in line:
            egl.append(values(line.partition("EGL presentation:")[2]))
    configs = {(s.get("core"), s.get("runahead_requested"), s.get("pacing_mode"))
               for s in samples}
    cores = sorted({v[0] for v in configs if v[0]})
    ras = sorted({v[1] for v in configs if v[1]})
    modes = sorted({v[2] for v in configs if v[2]})
    issues = []
    cautions = []
    expected_duration = report.get("requested_duration_s")
    try:
        actual_duration = float(report.get("duration_s", 0))
    except (TypeError, ValueError):
        actual_duration = 0.0
    if not samples:
        issues.append("Aucun jeu mesuré : démarrer le jeu avant la capture.")
    if len(configs) > 1:
        issues.append("Réglages modifiés pendant la capture : essai non comparable.")
    if expected_core is not None and cores != [expected_core]:
        issues.append(f"Core attendu {expected_core} ; trouvé {cores or 'aucun'}.")
    if expected_ra is not None and ras != [str(expected_ra)]:
        issues.append(f"Run-Ahead attendu {expected_ra} ; trouvé {ras or 'aucun'}.")
    if expected_duration and actual_duration < float(expected_duration) * 0.8:
        issues.append("Capture partielle : durée mesurée insuffisante.")
    underruns = report.get("audiotrack_underruns_delta")
    failures = report.get("runahead_failures_total")
    if underruns is None:
        issues.append("Compteur d'underruns absent.")
    elif underruns > 0:
        issues.append(f"{underruns} underruns AudioTrack confirmés.")
    if failures is None:
        cautions.append("Compteur d'échecs Run-Ahead absent.")
    elif failures > 0:
        issues.append(f"{failures} échecs Run-Ahead détectés.")
    if not depths:
        cautions.append("Réserve AudioTrack non disponible dans le logcat.")
    elif min(depths) < 1:
        cautions.append("Réserve AudioTrack <1 ms : risque sur une session prolongée.")
    elif min(depths) < 5:
        cautions.append("Réserve AudioTrack <5 ms : marge faible.")
    if egl and any(int(e.get("errors", "0")) or int(e.get("lost", "0"))
                   for e in egl):
        cautions.append("Timestamps EGL incomplets : FPS de présentation non fiables.")
    if not issues:
        status = "SURVEILLER" if cautions else "OK PROVISOIRE"
    else:
        status = "A REVOIR"
    return {
        "status": status, "core": cores, "runahead": ras, "pacing": modes,
        "duration_s": actual_duration, "windows": len(samples),
        "emu_fps_mean": (report.get("emu_fps") or {}).get("mean"),
        "gl_callback_fps_mean": (report.get("gl_callback_fps") or {}).get("mean"),
        "gl_over25": report.get("gl_over25_total"),
        "underruns": underruns, "runahead_failures": failures,
        "output_min_ms": round(min(depths), 3) if depths else None,
        "output_median_min_ms": round(median(depths), 3) if depths else None,
        "output_windows_below_1ms": sum(n < 1 for n in depths),
        "output_windows_below_5ms": sum(n < 5 for n in depths),
        "output_windows": len(depths), "early_pcm_wakes": sum(early) if early else None,
        "issues": issues, "cautions": cautions,
        "limits": [
            "Ne mesure pas la fluidité perçue ni la latence entrée->écran/son.",
            "Les FPS GL ne mesurent pas les images réellement affichées.",
            "Un essai sans underrun n'assure pas une stabilité infinie.",
            "audio_empty_total et queued_shorts ne mesurent pas les underruns AudioTrack.",
            "Ne remplace pas les tests sauvegardes, audio audible, lien et RetroAchievements.",
        ],
    }


def markdown(v: dict) -> str:
    fmt = lambda x: "n/d" if x is None else str(x)
    lines = [
        "# slot. — bilan automatique du test",
        "",
        f"**Résultat : {v['status']}**",
        "",
        f"- Configuration mesurée : {', '.join(v['core']) or '?'} / "
        f"Run-Ahead {', '.join(v['runahead']) or '?'} / "
        f"{', '.join(v['pacing']) or '?'}",
        f"- Durée : {v['duration_s']} s ; {v['windows']} fenêtres",
        f"- FPS émulation moyen (échantillonné) : {fmt(v['emu_fps_mean'])}",
        f"- FPS callbacks GL moyen : {fmt(v['gl_callback_fps_mean'])}",
        f"- Callbacks GL >25 ms : {fmt(v['gl_over25'])}",
        f"- **Underruns AudioTrack : {fmt(v['underruns'])}**",
        f"- Échecs Run-Ahead : {fmt(v['runahead_failures'])}",
        f"- Réserve AudioTrack minimale : {fmt(v['output_min_ms'])} ms",
        f"- Médiane des minima par fenêtre : {fmt(v['output_median_min_ms'])} ms",
        f"- Fenêtres avec réserve <1 ms : {v['output_windows_below_1ms']}/{v['output_windows']}",
        f"- Fenêtres avec réserve <5 ms : {v['output_windows_below_5ms']}/{v['output_windows']}",
        f"- Réveils audio anticipés : {fmt(v['early_pcm_wakes'])}",
    ]
    if v["issues"]:
        lines += ["", "**Points bloquants**"] + ["- " + x for x in v["issues"]]
    if v["cautions"]:
        lines += ["", "**À surveiller**"] + ["- " + x for x in v["cautions"]]
    lines += ["", "**Limites**"] + ["- " + x for x in v["limits"]]
    return "\n".join(lines) + "\n"


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--serial", default="BW0308N250009576")
    p.add_argument("--seconds", type=int, default=180)
    p.add_argument("--kit", type=Path, default=Path.cwd(),
                   help="Dossier qui contient tools/emulation-lab/capture.sh")
    p.add_argument("--existing", type=Path,
                   help="Analyser un dossier de capture existant, sans lancer ADB")
    p.add_argument("--expect-core", choices=["mgba", "gpsp"])
    p.add_argument("--expect-ra", type=int, choices=[0, 1, 2])
    args = p.parse_args()
    if args.existing:
        out = args.existing.resolve()
    else:
        if not SAFE.fullmatch(args.serial):
            p.error("Numéro de série ADB invalide.")
        if not 10 <= args.seconds <= 600:
            p.error("Durée autorisée : 10 à 600 secondes.")
        kit = args.kit.expanduser().resolve()
        capture = kit / "tools" / "emulation-lab" / "capture.sh"
        if not capture.is_file():
            p.error(f"capture.sh absent : {capture}")
        unique = datetime.now(timezone.utc).strftime("auto_%Y%m%dT%H%M%SZ")
        print(f"Capture en cours : {args.seconds}s, jeu déjà lancé sur {args.serial}", flush=True)
        completed = subprocess.run(["bash", str(capture), args.serial, unique,
                                    str(args.seconds)], cwd=kit)
        if completed.returncode:
            return completed.returncode
        candidates = sorted((kit / "emulation-lab-results").glob("*-" + unique))
        if len(candidates) != 1:
            print("Dossier de capture introuvable/ambigu.", file=sys.stderr)
            return 2
        out = candidates[0]
    try:
        report = json.loads((out / "report.json").read_text(encoding="utf8"))
        logcat = (out / "logcat.txt").read_text(encoding="utf8", errors="replace")
    except (OSError, ValueError) as exc:
        print(f"Données de capture invalides : {exc}", file=sys.stderr)
        return 2
    result = triage(report, logcat, args.expect_core, args.expect_ra)
    (out / "quickcheck.json").write_text(
        json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf8")
    report_md = markdown(result)
    (out / "quickcheck.md").write_text(report_md, encoding="utf8")
    print("\n" + report_md)
    print(f"Rapport enregistré : {out / 'quickcheck.md'}")
    # Failure is a signal for automation, not a claim about audible quality.
    return 1 if result["issues"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
