"""Recompose Watomsk's 1920x1080 overlays for other panels.

Every output keeps Watomsk's own pieces (logos, LEDs, strips, inner
shadow) at his pixel scale, on a canvas whose transparent window is the
exact rectangle slot. draws the game in.
"""
import numpy as np
from PIL import Image

SRC = None  # set by caller: dir with console folders (1080p)


def load(rel):
    im = Image.open(f"{SRC}/{rel}").convert("RGBA")
    a = np.array(im)
    al = a[..., 3]
    z = np.argwhere(al == 0)
    (y0, x0), (y1, x1) = z.min(0), z.max(0)
    cy, cx = (y0 + y1) // 2, (x0 + x1) // 2
    L = cx
    while L > 0 and al[cy, L - 1] < 255:
        L -= 1
    R = cx
    while R < al.shape[1] - 1 and al[cy, R + 1] < 255:
        R += 1
    T = cy
    while T > 0 and al[T - 1, cx] < 255:
        T -= 1
    B = cy
    while B < al.shape[0] - 1 and al[B + 1, cx] < 255:
        B += 1
    rect = (L, T, R + 1, B + 1)
    prof = al[cy, L:L + 48].astype(np.float32)
    prof = np.concatenate([prof, np.zeros(4, np.float32)])
    flat = Image.new("RGBA", im.size, (0, 0, 0, 255))
    flat.alpha_composite(im)
    flat = np.array(flat)[..., :3].astype(np.int16)
    bg = tuple(int(v) for v in flat[5, 5])
    return dict(im=im, flat=flat, bg=bg, rect=rect, prof=prof)


def strong_bbox(flat, bg, box, thr=40):
    x0, y0, x1, y1 = box
    sub = flat[y0:y1, x0:x1]
    m = np.abs(sub - np.array(bg)).max(axis=2) > thr
    if not m.any():
        return None
    ys, xs = np.where(m)
    return (x0 + xs.min(), y0 + ys.min(), x0 + xs.max() + 1, y0 + ys.max() + 1)


def crop_rgb(src, box):
    x0, y0, x1, y1 = box
    return Image.fromarray(src["flat"][y0:y1, x0:x1].astype(np.uint8), "RGB")


class Canvas:
    def __init__(self, W, H, rect, src, shadow_scale):
        self.W, self.H, self.rect, self.src = W, H, rect, src
        self.rgb = Image.new("RGB", (W, H), src["bg"])
        self.shadow_scale = shadow_scale

    def paste(self, patch, x, y, clip=None):
        """Paste an opaque RGB patch, clipped to `clip` (default: canvas
        minus the game window)."""
        x, y = int(round(x)), int(round(y))
        cx0, cy0, cx1, cy1 = clip or (0, 0, self.W, self.H)
        px0, py0 = max(x, cx0), max(y, cy0)
        px1, py1 = min(x + patch.width, cx1), min(y + patch.height, cy1)
        if px1 <= px0 or py1 <= py0:
            return
        part = patch.crop((px0 - x, py0 - y, px1 - x, py1 - y))
        if clip is None:
            # Never draw over the game window.
            m = Image.new("L", part.size, 255)
            rx0, ry0, rx1, ry1 = self.rect
            ix0, iy0 = max(rx0, px0), max(ry0, py0)
            ix1, iy1 = min(rx1, px1), min(ry1, py1)
            if ix1 > ix0 and iy1 > iy0:
                m.paste(0, (ix0 - px0, iy0 - py0, ix1 - px0, iy1 - py0))
            self.rgb.paste(part, (px0, py0), m)
        else:
            self.rgb.paste(part, (px0, py0))

    def paste_soft(self, patch, x, y, clip):
        """Paste a glowing piece (LED) with a round fade, so its crop edge
        never shows."""
        w, h = patch.size
        yy, xx = np.mgrid[0:h, 0:w]
        r = np.hypot((xx - (w - 1) / 2) / (w / 2), (yy - (h - 1) / 2) / (h / 2))
        m = np.clip((1 - r) / 0.45, 0, 1)
        x, y = int(round(x)), int(round(y))
        under = self.rgb.crop((x, y, x + w, y + h))
        mixed = Image.composite(patch, under, Image.fromarray((m * 255).astype(np.uint8), "L"))
        self.paste(mixed, x, y, clip=clip)

    def finish(self):
        W, H = self.W, self.H
        x0, y0, x1, y1 = self.rect
        prof = self.src["prof"]
        s = self.shadow_scale
        alpha = np.full((H, W), 255, np.float32)
        yy, xx = np.mgrid[y0:y1, x0:x1]
        d = np.stack([xx - x0, x1 - 1 - xx, yy - y0, y1 - 1 - yy]) / s
        d = np.clip(d, 0, len(prof) - 1)
        v = np.interp(d, np.arange(len(prof)), prof).max(axis=0)
        alpha[y0:y1, x0:x1] = v
        rgb = np.array(self.rgb)
        rgb[y0:y1, x0:x1] = 0  # Watomsk's shadow is black.
        out = np.dstack([rgb, alpha.round().clip(0, 255).astype(np.uint8)])
        return Image.fromarray(out, "RGBA")


def resize(img, s):
    w, h = max(1, round(img.width * s)), max(1, round(img.height * s))
    return img.resize((w, h), Image.LANCZOS)


# --- panel groups -----------------------------------------------------------

def side_panels(src):
    """Source left / right panel boxes (full height)."""
    x0, _, x1, _ = src["rect"]
    H = src["flat"].shape[0]
    W = src["flat"].shape[1]
    return (0, 0, x0, H), (x1, 0, W, H)


def place_panel_group(cv, src, box, tgt, s, center_y_scale, pad=0.16):
    """Move everything drawn in source panel `box` into target panel
    `tgt`, at scale s (shrunk further to fit), centred horizontally on the
    strong content; vertical position follows the source (scaled)."""
    sb = strong_bbox(src["flat"], src["bg"], box)
    if sb is None:
        return
    tw = tgt[2] - tgt[0]
    sw = sb[2] - sb[0]
    s2 = min(s, tw * (1 - pad) / sw)
    patch = resize(crop_rgb(src, box), s2)
    scx = ((sb[0] + sb[2]) / 2 - box[0]) * s2
    scy = ((sb[1] + sb[3]) / 2 - box[1]) * s2
    tcx = (tgt[0] + tgt[2]) / 2
    tcy = tgt[1] + ((sb[1] + sb[3]) / 2 - box[1]) * center_y_scale
    cv.paste(patch, tcx - scx, tcy - scy, clip=tgt)


# --- Sega power strip --------------------------------------------------------

def strip_info(src):
    """Band rows, LED x and a text-free copy of the left panel."""
    (l, _, r, H), _ = side_panels(src)
    flat = src["flat"]
    col = flat[:, 4]
    band = np.where(np.abs(col - np.array(src["bg"])).max(axis=1) > 25)[0]
    by0, by1 = band.min(), band.max() + 1
    rows = flat[by0:by1, :r]
    # LED = brightest red spot inside the band.
    red = rows[..., 0].astype(np.int32) - rows[..., 1:].mean(axis=2)
    lum = rows.mean(axis=2)
    score = red + lum
    ly, lx = np.unravel_index(np.argmax(score[:, 20:r - 20]), score[:, 20:r - 20].shape)
    lx += 20
    # Text and line run through the middle rows: rebuild them from the
    # rows just above / below, except across the LED itself.
    clean = flat[:, :r].copy()
    m = 7
    top, bot = by0 + m, by1 - m
    for x in range(r):
        if abs(x - lx) <= 16:
            continue
        a, b = clean[top - 1, x].astype(np.float32), clean[bot, x].astype(np.float32)
        for y in range(top, bot):
            t = (y - top + 1) / (bot - top + 1)
            clean[y, x] = (a * (1 - t) + b * t).round()
    return dict(by0=by0, by1=by1, lx=lx, clean=clean, panel_w=r)


def place_strip_side(cv, src, si, tgt, s):
    """Strip across a target left panel (edge to window)."""
    tw = tgt[2] - tgt[0]
    full = crop_rgb(src, (0, 0, si["panel_w"], src["flat"].shape[0]))
    if si["panel_w"] * s <= tw * 1.25:
        p = resize(full, s)
        # Extend the band to the left edge with its own first column.
        ext = p.crop((0, 0, 1, p.height)).resize((tw, p.height))
        cv.paste(ext, tgt[0], 0, clip=tgt)
        cv.paste(p, tgt[2] - p.width, 0, clip=tgt)
    else:
        clean = Image.fromarray(si["clean"].astype(np.uint8), "RGB")
        p = resize(clean, s)
        cv.paste(p, (tgt[0] + tgt[2]) / 2 - si["lx"] * s, 0, clip=tgt)


def place_strip_band(cv, src, si, x, cy, s, clip):
    pad = 110
    box = (0, si["by0"] - pad, si["panel_w"], si["by1"] + pad)
    p = resize(crop_rgb(src, box), s)
    bcy = ((si["by0"] + si["by1"]) / 2 - box[1]) * s
    cv.paste(p, x, cy - bcy, clip=clip)
    return p.width
