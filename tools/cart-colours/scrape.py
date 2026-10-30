"""Measures the real plastic of every GB/GBC/GBA cartridge from ScreenScraper's support-2D media.

One pass, one request at a time, stopping for the day on 429/430/431. Images stay in the
workflow cache: only the measured numbers (results.jsonl) leave the run.
"""
import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

DAT_BASE = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/"
SYSTEMS = {
    "GB": ("Nintendo - Game Boy.dat", 9),
    "GBC": ("Nintendo - Game Boy Color.dat", 10),
    "GBA": ("Nintendo - Game Boy Advance.dat", 12),
}
SKIP = re.compile(r"\((Proto|Beta|Demo|Sample|Debug|Program|Pirate|Aftermarket)[^)]*\)|\[BIOS\]", re.I)
REGIONS = {
    "World": "wor", "USA": "us", "Europe": "eu", "Japan": "jp", "France": "fr",
    "Germany": "de", "Spain": "sp", "Italy": "it", "Netherlands": "nl", "Sweden": "se",
    "Australia": "au", "Korea": "kr", "China": "cn", "Canada": "us", "Brazil": "br",
    "Asia": "asi", "Taiwan": "tw", "UK": "uk",
}
STOP = {429, 430, 431}


def env(name):
    return os.environ.get(name, "")


def parse_dat(text):
    for block in re.finditer(r"game \((.*?)\n\)", text, re.S):
        body = block.group(1)
        name = re.search(r'name "([^"]+)"', body)
        rom = re.search(r"rom \((.*?)\)\s*$", body, re.M | re.S)
        if not name or not rom:
            continue
        r = rom.group(1)
        crc = re.search(r"crc ([0-9A-Fa-f]{8})", r)
        size = re.search(r"size (\d+)", r)
        serial = re.search(r'serial "([^"]+)"', r)
        if crc and size:
            yield {
                "name": name.group(1),
                "crc": crc.group(1).upper(),
                "size": int(size.group(1)),
                "serial": serial.group(1) if serial else "",
            }


def regions_of(name):
    found = []
    for group in re.findall(r"\(([^)]*)\)", name):
        for part in group.split(","):
            code = REGIONS.get(part.strip())
            if code and code not in found:
                found.append(code)
    return found


def get(url, timeout=60):
    req = urllib.request.Request(url, headers={"User-Agent": "slot-cart-colours"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def jeu_infos(system_id, rom):
    q = {
        "devid": env("SS_DEVID"), "devpassword": env("SS_DEVPASSWORD"),
        "softname": env("SS_SOFTNAME"), "output": "json",
        "systemeid": system_id, "romtype": "rom",
        "crc": rom["crc"], "romtaille": rom["size"], "romnom": rom["name"],
    }
    if env("SS_USER"):
        q["ssid"], q["sspassword"] = env("SS_USER"), env("SS_PASSWORD")
    return get("https://api.screenscraper.fr/api2/jeuInfos.php?" + urllib.parse.urlencode(q))


def pick_support(medias, wanted):
    supports = [m for m in medias if m.get("type") == "support-2D" and m.get("url")]
    if not supports:
        return None
    by_region = {m.get("region", ""): m for m in supports}
    for region in [*wanted, "wor", "eu", "us", "jp", "ss", ""]:
        if region in by_region:
            return by_region[region]
    return supports[0]


def measure(path):
    """Shell colour and texture from a band just inside the cartridge's outline."""
    img = Image.open(path).convert("RGBA")
    scale = 400 / max(img.size)
    if scale < 1:
        img = img.resize((round(img.width * scale), round(img.height * scale)), Image.LANCZOS)
    a = np.asarray(img).astype(np.float32)
    rgb, alpha = a[..., :3], a[..., 3]
    if alpha.min() < 250:
        mask = alpha > 200
    else:
        # Opaque picture: the background is whatever touches the corners.
        corners = np.array([rgb[0, 0], rgb[0, -1], rgb[-1, 0], rgb[-1, -1]])
        bg = np.median(corners, axis=0)
        mask = np.abs(rgb - bg).sum(axis=2) > 30
    mask = ndimage.binary_fill_holes(ndimage.binary_opening(mask, iterations=2))
    labels, n = ndimage.label(mask)
    if n == 0:
        return None
    sizes = ndimage.sum(mask, labels, range(1, n + 1))
    mask = labels == (1 + int(np.argmax(sizes)))
    ys, xs = np.nonzero(mask)
    w, h = xs.max() - xs.min() + 1, ys.max() - ys.min() + 1
    depth = ndimage.distance_transform_edt(mask)
    unit = min(w, h)
    band = (depth > unit * 0.03) & (depth < unit * 0.09)
    if band.sum() < 200:
        return None
    px = rgb[band]
    lum = px @ np.array([0.299, 0.587, 0.114])
    lo, hi = np.percentile(lum, [20, 80])
    core = px[(lum >= lo) & (lum <= hi)]
    colour = np.median(core, axis=0)
    # Glitter: isolated bright points well above their neighbourhood.
    grey = rgb @ np.array([0.299, 0.587, 0.114])
    local = ndimage.median_filter(grey, size=7)
    flecks = ((grey - local) > 35) & band
    # Clear plastic lets the board's greens and golds through: wide hue spread in the band.
    hsv = np.asarray(Image.fromarray(rgb.astype(np.uint8)).convert("HSV")).astype(np.float32)
    sat = hsv[..., 1][band]
    return {
        "colour": [int(round(c)) for c in colour],
        "lum_std": round(float(lum.std()), 2),
        "chroma_std": round(float(np.std(px - px.mean(axis=0), axis=0).mean()), 2),
        "sat_mean": round(float(sat.mean()), 2),
        "sat_std": round(float(sat.std()), 2),
        "fleck_rate": round(float(flecks.sum() / band.sum()), 5),
        "alpha_mean": round(float(alpha[mask].mean()), 1),
        "aspect": round(float(h / w), 3),
        # A coarse 12x12 colour grid, enough to tell a template from a photo when reviewing.
        "grid": np.asarray(img.convert("RGB").resize((12, 12), Image.BOX)).reshape(-1, 3).tolist(),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default="cache")
    ap.add_argument("--out", default="out")
    ap.add_argument("--budget-min", type=float, default=320)
    ap.add_argument("--limit", type=int, default=0, help="max new ROMs per system (0 = all)")
    ap.add_argument("--pause", type=float, default=1.2)
    ap.add_argument("--only", default="", help="regex of ROM names to re-check, printing their media")
    args = ap.parse_args()
    cache, out = Path(args.cache), Path(args.out)
    (cache / "img").mkdir(parents=True, exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    seen_path = cache / "seen.jsonl"
    seen = {}
    if seen_path.exists():
        for line in seen_path.read_text().splitlines():
            row = json.loads(line)
            seen[(row["platform"], row["crc"])] = row
    deadline = time.time() + args.budget_min * 60
    stopped = None
    with seen_path.open("a") as log:
        for platform, (dat_name, system_id) in SYSTEMS.items():
            status, body = get(DAT_BASE + urllib.parse.quote(dat_name))
            if status != 200:
                print(f"{platform}: DAT HTTP {status}", file=sys.stderr)
                continue
            roms = [r for r in parse_dat(body.decode("utf-8", "replace")) if not SKIP.search(r["name"])]
            new = 0
            print(f"{platform}: {len(roms)} ROMs, {sum((platform, r['crc']) in seen for r in roms)} done")
            if args.only:
                roms = [r for r in roms if re.search(args.only, r["name"])]
                seen = {k: v for k, v in seen.items() if not re.search(args.only, v["name"])}
            for rom in roms:
                if (platform, rom["crc"]) in seen:
                    continue
                if stopped or time.time() > deadline or (args.limit and new >= args.limit):
                    break
                new += 1
                row = {"platform": platform, **rom, "regions": regions_of(rom["name"])}
                status, body = jeu_infos(system_id, rom)
                time.sleep(args.pause)
                if status in STOP:
                    stopped = f"HTTP {status}: {body[:120]!r}"
                    break
                if status != 200:
                    row["error"] = status
                else:
                    try:
                        jeu = json.loads(body)["response"]["jeu"]
                    except (ValueError, KeyError):
                        row["error"] = "bad json"
                        jeu = None
                    if jeu:
                        if args.only:
                            kinds = sorted({f"{m.get('type')}:{m.get('region', '')}" for m in jeu.get("medias") or []})
                            print("MEDIA", rom["name"], " ".join(kinds))
                        row["game_id"] = jeu.get("id")
                        names = jeu.get("noms") or []
                        row["ss_name"] = next((n.get("text") for n in names if n.get("region") in ("wor", "eu", "us")), names[0].get("text") if names else "")
                        media = pick_support(jeu.get("medias") or [], row["regions"])
                        if media:
                            row["media_region"] = media.get("region", "")
                            key = media.get("md5") or media.get("crc") or f"{row['game_id']}-{row['media_region']}"
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
                        else:
                            row["image"] = None
                log.write(json.dumps(row) + "\n")
                log.flush()
                seen[(platform, rom["crc"])] = row
            if stopped:
                break
    print("stopped:", stopped or "no")
    results = []
    for row in seen.values():
        row = dict(row)
        if row.get("image"):
            try:
                row["measure"] = measure(cache / "img" / row["image"])
            except Exception as e:  # a broken download must not lose the rest
                row["measure_error"] = str(e)
        results.append(row)
    with (out / "results.jsonl").open("w") as f:
        for row in results:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
    # Artifacts can't be fetched from everywhere the table is reviewed; the log can.
    for row in results:
        m = row.get("measure") or {}
        print("ROW", json.dumps({
            "p": row["platform"], "crc": row["crc"], "serial": row["serial"], "n": row["name"],
            "r": row.get("media_region"), "e": row.get("error"), "m": m or None,
        }, ensure_ascii=False, separators=(",", ":")))
    with_image = sum(1 for r in results if r.get("image"))
    print(f"rows: {len(results)}, with support-2D: {with_image}")
    (out / "summary.txt").write_text(f"rows {len(results)}\nwith_support {with_image}\nstopped {stopped}\n")


if __name__ == "__main__":
    main()
