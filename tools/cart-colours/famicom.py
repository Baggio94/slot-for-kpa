"""Measures the shell colour of every Famicom cartridge from ScreenScraper's support-2D.

Unlike the Game Boy family, ScreenScraper draws Famicom carts in their own plastic
(Super Mario Bros. comes back orange, 195,124,7), so support-2D is enough here.
One request at a time, one cartridge per title (revisions share the shell), stopping
for the day on 429/430/431. Images stay in the workflow cache; only numbers are logged.
"""
import argparse
import json
import re
import sys
import time
import urllib.parse
from pathlib import Path

from scrape import DAT_BASE, SKIP, STOP, get, jeu_infos, measure, parse_dat, pick_support, regions_of

NES_DAT, NES_ID = "Nintendo - Nintendo Entertainment System.dat", 3
EXTRA_SKIP = re.compile(r"\((Unl|Hack|Virtual Console|Namcot Collection|Switch Online)[^)]*\)", re.I)


def title_key(name):
    return re.sub(r"\s*\((Rev [^)]*|v\d[^)]*|Alt[^)]*)\)", "", name).strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default="cache")
    ap.add_argument("--out", default="out")
    ap.add_argument("--budget-min", type=float, default=320)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--pause", type=float, default=1.2)
    args = ap.parse_args()
    cache, out = Path(args.cache), Path(args.out)
    (cache / "img").mkdir(parents=True, exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    seen_path = cache / "seen-fc.jsonl"
    seen = {}
    if seen_path.exists():
        for line in seen_path.read_text().splitlines():
            row = json.loads(line)
            seen[row["key"]] = row
    status, body = get(DAT_BASE + urllib.parse.quote(NES_DAT))
    if status != 200:
        sys.exit(f"DAT HTTP {status}")
    roms, keys = [], set()
    for r in parse_dat(body.decode("utf-8", "replace")):
        if SKIP.search(r["name"]) or EXTRA_SKIP.search(r["name"]) or regions_of(r["name"]) != ["jp"]:
            continue
        k = title_key(r["name"])
        if k not in keys:
            keys.add(k)
            roms.append({**r, "key": k})
    print(f"FC: {len(roms)} titles, {sum(r['key'] in seen for r in roms)} done")
    deadline = time.time() + args.budget_min * 60
    stopped, new = None, 0
    with seen_path.open("a") as log:
        for rom in roms:
            if rom["key"] in seen:
                continue
            if time.time() > deadline or (args.limit and new >= args.limit):
                break
            new += 1
            row = dict(rom)
            status, body = jeu_infos(NES_ID, rom)
            time.sleep(args.pause)
            if status in STOP:
                stopped = f"HTTP {status}: {body[:120]!r}"
                break
            jeu = None
            if status == 200:
                try:
                    jeu = json.loads(body)["response"]["jeu"]
                except (ValueError, KeyError):
                    row["error"] = "bad json"
            else:
                row["error"] = status
            if jeu:
                row["game_id"] = jeu.get("id")
                medias = jeu.get("medias") or []
                row["kinds"] = sorted({f"{m.get('type')}:{m.get('region', '')}" for m in medias
                                       if str(m.get("type", "")).startswith("support")})
                media = pick_support(medias, ["jp"], "support-2D")
                if media:
                    row["media_region"] = media.get("region", "")
                    key = media.get("md5") or media.get("crc") or f"{row['game_id']}-2d-{row['media_region']}"
                    img = cache / "img" / f"{key}.{media.get('format', 'png')}"
                    if not img.exists():
                        st, data = get(media["url"], timeout=120)
                        time.sleep(args.pause)
                        if st in STOP:
                            stopped = f"media HTTP {st}"
                            break
                        if st == 200 and data[:4] != b"NOME":
                            img.write_bytes(data)
                    row["image"] = img.name if img.exists() else None
            log.write(json.dumps(row) + "\n")
            log.flush()
            seen[rom["key"]] = row
    print("stopped:", stopped or "no")
    results = []
    for row in seen.values():
        row = dict(row)
        if row.get("image"):
            try:
                row["measure"] = measure(cache / "img" / row["image"])
            except Exception as e:
                row["measure_error"] = str(e)
        results.append(row)
    with (out / "famicom.jsonl").open("w") as f:
        for row in results:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
    # Compact rows for the log (artifacts can't be fetched everywhere the table is reviewed).
    for row in results:
        m = row.get("measure") or {}
        print("FC", json.dumps({
            "crc": row["crc"], "n": row["name"], "r": row.get("media_region"), "e": row.get("error"),
            "k": row.get("kinds"), "c": m.get("colour"), "a": m.get("aspect"), "f": m.get("fleck_rate"),
            "ls": m.get("lum_std"), "cs": m.get("chroma_std"), "img": row.get("image"),
        }, ensure_ascii=False, separators=(",", ":")))
    print(f"titles: {len(results)}, with support-2D: {sum(1 for r in results if r.get('image'))}")


if __name__ == "__main__":
    main()
