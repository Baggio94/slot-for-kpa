"""Export the Watomsk overlay masters slot. ships (assets/watomsk/).

Usage: python3 export.py <Watomsk 1080p dir> <assets/watomsk dir>

<1080p dir> holds his console folders (GameBoy, GameBoyAdvance, NES, SNES,
Genesis) from WatomsksOverlays-1080p. Only his black variants ship (the
dark grey -G ones were dropped, Siham 2026-11-04). Two kinds of files:

* <name>-<3x2|4x3|16x9>.png: side frames, all 1080 high (his own
  resolution, so 3x panels stay sharp), the window cut for the full
  height at the system's shape (GB 10:9, GBA 3:2, home 4:3), centred. The
  app refits them to the real panel (fit_overlay).
* <name>-band.png: the logo (opaque, on the frame's colour) for screens
  narrower than the picture: the game sits at the top and the logo is
  centred in the band below (fit_band). Drawn at his 1080p scale.

The Game Boy 3:2 frames are make.py's (validated on the KONKR); GBA has no
LED anywhere and the Mega Drive band drops the POWER strip (Siham,
2026-11-03).
"""
import os
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import compose as C  # noqa: E402
import make  # noqa: E402

VARIANTS = {
    "GameBoy": [("GB", "gb"), ("GBP", "gbp"), ("GBC", "gbc")],
    "GameBoyAdvance": [("GBA-Classic", "gba-classic"), ("GBA", "gba"),
                       ("GBASP", "gbasp"), ("GBA-Alt", "gba-alt")],
    "NES": [("NES", "nes"), ("NES-GrayLogo", "nes-graylogo")],
    "SNES": [("SNES", "snes"), ("SNES-Alt", "snes-alt"),
             ("SNES-GrayLogo", "snes-graylogo"), ("SNES-GrayLogo-Alt", "snes-graylogo-alt")],
    "Genesis": [("Genesis-Classic", "genesis-classic"), ("Genesis", "genesis"),
                ("Megadrive", "megadrive")],
}
SHAPE = {"GameBoy": 10 / 9, "GameBoyAdvance": 1.5, "NES": 4 / 3, "SNES": 4 / 3, "Genesis": 4 / 3}


def side(src, W, H, aspect, logo=None):
    w = round(H * aspect)
    rect = ((W - w) // 2, 0, (W - w) // 2 + w, H)
    s = H / 1080
    cv = C.Canvas(W, H, rect, src, s)
    sl, sr = C.side_panels(src)
    tl, tr = (0, 0, rect[0], H), (rect[2], 0, W, H)
    if src.get("sega"):
        C.place_strip_side(cv, src, src["sega"], tl, s)
    elif logo is None:
        C.place_panel_group(cv, src, sl, tl, s, s)
    if logo is not None:
        lg = C.resize(logo, s)
        tw = tr[2] - tr[0]
        if lg.width > tw * 0.8:
            lg = C.resize(lg, tw * 0.8 / lg.width)
        cv.paste(lg, (tr[0] + tr[2] - lg.width) / 2, (H - lg.height) / 2, clip=tr)
    else:
        C.place_panel_group(cv, src, sr, tr, s, s)
    return cv.finish()


def centred_crop(src, box, centre):
    """Crop `box` grown so `centre` (source px) is the crop's centre."""
    x0, y0, x1, y1 = box
    cx, cy = centre
    hw = max(cx - x0, x1 - cx)
    hh = max(cy - y0, y1 - cy)
    bx0, by0 = round(cx - hw), round(cy - hh)
    bx1, by1 = round(cx + hw), round(cy + hh)
    out = Image.new("RGB", (bx1 - bx0, by1 - by0), src["bg"])
    H, W = src["flat"].shape[:2]
    inner = (max(bx0, 0), max(by0, 0), min(bx1, W), min(by1, H))
    out.paste(C.crop_rgb(src, inner), (inner[0] - bx0, inner[1] - by0))
    return out


def gba_logo(src):
    x0, y0, x1, y1 = src["rect"]
    lb = C.strong_bbox(src["flat"], src["bg"], (0, y1, 1920, 1080))
    pad = 8
    box = (lb[0] - pad, lb[1] - pad, lb[2] + pad, min(1080, lb[3] + pad))
    return centred_crop(src, box, ((lb[0] + lb[2]) / 2, (lb[1] + lb[3]) / 2))


def sega_logo(src):
    """The vertical logo with its grid, turned to read left to right."""
    _, sr = C.side_panels(src)
    sb = C.strong_bbox(src["flat"], src["bg"], sr)
    box = (sr[0] + 20, 0, 1920, 620)
    centre = ((sb[0] + sb[2]) / 2, (sb[1] + sb[3]) / 2)
    return centred_crop(src, box, centre).rotate(90, expand=True)


def save(img, path):
    img.save(path, optimize=True)


def main(src_dir, out):
    C.SRC = src_dir
    os.makedirs(out, exist_ok=True)
    for console, vs in VARIANTS.items():
        for name, key in vs:
            src = C.load(f"{console}/{name}.png")
            if console == "Genesis":
                src["sega"] = C.strip_info(src)
            tag = key
            shape = SHAPE[console]
            if console == "GameBoy":
                save(make.adapt(f"{src_dir}/{console}/{name}.png", (1620, 1080)),
                     f"{out}/{tag}-3x2.png")
                save(side(src, 1440, 1080, shape), f"{out}/{tag}-4x3.png")
                save(src["im"], f"{out}/{tag}-16x9.png")
            elif console == "GameBoyAdvance":
                logo = gba_logo(src)
                save(side(src, 1920, 1080, shape, logo.rotate(-90, expand=True)),
                     f"{out}/{tag}-16x9.png")
                save(logo, f"{out}/{tag}-band.png")
            else:
                save(side(src, 1620, 1080, shape), f"{out}/{tag}-3x2.png")
                save(side(src, 1920, 1080, shape), f"{out}/{tag}-16x9.png")
                if console == "Genesis":
                    save(sega_logo(src), f"{out}/{tag}-band.png")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
