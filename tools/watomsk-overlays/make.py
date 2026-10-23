"""Cut Watomsk's 1920x1080 Game Boy overlays to the KONKR's 3:2 panel.

Usage: python3 make.py <dir with GB.png GBP.png GBC.png> <assets dir>

The 10:9 window (1200x1080, full height) is kept as is; each black side
panel is narrowed from 360 to 210 px and its LED / logo re-centred, then
the 1620x1080 result is resized to 960x640.
"""
import sys

import numpy as np
from PIL import Image

HOLE = (360, 1560)
SIDE = (1620 - 1200) // 2


def adapt(path):
    im = Image.open(path).convert("RGBA")
    out = Image.new("RGBA", (1620, 1080), (0, 0, 0, 255))
    out.paste(im.crop((HOLE[0], 0, HOLE[1], 1080)), (SIDE, 0))
    for a, b, dst in [(0, HOLE[0], 0), (HOLE[1], 1920, SIDE + 1200)]:
        panel = im.crop((a, 0, b, 1080))
        arr = np.array(panel)
        mask = (arr[..., :3].max(axis=2) > 24) & (arr[..., 3] > 40)
        # The window's inner shadow is not panel content.
        if a == 0:
            mask[:, -30:] = False
        else:
            mask[:, :30] = False
        xs = np.where(mask.any(axis=0))[0]
        x0, x1 = xs.min(), xs.max() + 1
        out.alpha_composite(panel.crop((x0, 0, x1, 1080)), (dst + (SIDE - (x1 - x0)) // 2, 0))
    return out.resize((960, 640), Image.LANCZOS)


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    for name, asset in [("GB", "gb"), ("GBP", "pocket"), ("GBC", "color")]:
        adapt(f"{src}/{name}.png").save(f"{dst}/overlay_watomsk_{asset}.png", optimize=True)
