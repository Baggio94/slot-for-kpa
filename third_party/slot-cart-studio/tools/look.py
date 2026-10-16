"""A cart's printed label, measured from its ScreenScraper scan: the colour of its ground and at
most one flat band along an edge. Only the measurements are kept, never the image."""

import io

import numpy as np
from PIL import Image, UnidentifiedImageError

# The label's box in each ScreenScraper template, measured by overlaying scans of different games.
LABEL_BOX = {
    (600, 678): (91, 209, 504, 572),  # Game Boy pak
    (600, 355): (88, 86, 515, 302),  # GBA cart
    (600, 701): (97, 254, 519, 631),  # Game Boy Color pak, photographed
}
# Every Game Boy label is printed with a strip down each side ("THIS SIDE OUT", the licence code).
GAME_BOY = {(600, 678), (600, 701)}
SIDE_STRIP = 0.09

# A line is flat when most of it is one colour: lettering printed across a band is the rest.
FLAT_NEAR = 40.0
FLAT_SHARE = 0.75
# How far a line's colour may drift from the band's first line before the band ends.
SAME = 18.0
BAND_APART = 60.0
BAND_MIN, BAND_MAX = 0.03, 0.30
GROUND_SHARE = 0.45
BORDERLINE = 0.25
RIVAL = 0.70
RING = 0.10
CLUSTERS = 5
EDGES = ('top', 'bottom', 'left', 'right')


def _label(data):
    try:
        im = Image.open(io.BytesIO(data))
        im.load()
    except (UnidentifiedImageError, OSError):
        return None
    box = LABEL_BOX.get(im.size)
    if box is None:
        return None
    game_boy = im.size in GAME_BOY
    px = np.asarray(im.convert('RGB').crop(box), dtype=float)
    if game_boy:
        cut = round(px.shape[1] * SIDE_STRIP)
        px = px[:, cut:px.shape[1] - cut]
    return px, game_boy


def _paper(colour):
    """Whether a colour is the pale, near-grey paper a Game Boy label's side strips are printed on."""
    return colour.min() >= 200 and colour.max() - colour.min() <= 30


def _from(px, edge):
    """The label turned so that `edge` is row 0 and rows run inward from it. A view, so writing
    through it writes the label."""
    if edge == 'top':
        return px
    if edge == 'bottom':
        return px[::-1]
    turned = px.swapaxes(0, 1)
    return turned if edge == 'left' else turned[::-1]


def _flat(line):
    """The line's colour, and whether most of the line is that colour."""
    median = np.median(line, axis=0)
    return median, float((np.linalg.norm(line - median, axis=1) <= FLAT_NEAR).mean()) >= FLAT_SHARE


def _candidate(px, edge):
    rows = _from(px, edge)
    n = rows.shape[0]
    colour, flat = _flat(rows[0])
    if not flat:
        return None
    k = 1
    while k < n:
        median, flat = _flat(rows[k])
        if not flat or np.linalg.norm(median - colour) > SAME:
            break
        k += 1
    size = k / n
    if not BAND_MIN <= size <= BAND_MAX:
        return None
    depth = max(2, round(n * 0.05))
    inside = np.median(rows[k:k + depth].reshape(-1, 3), axis=0)
    apart = float(np.linalg.norm(colour - inside))
    return {'edge': edge, 'size': size, 'colour': colour, 'apart': apart, 'score': apart * size}


def _kmeans(pixels, k):
    order = np.argsort(pixels.sum(axis=1))
    centres = pixels[order[np.linspace(0, len(order) - 1, k).astype(int)]].copy()
    for _ in range(12):
        nearest = np.argmin(((pixels[:, None, :] - centres[None]) ** 2).sum(axis=2), axis=1)
        for i in range(k):
            members = pixels[nearest == i]
            if len(members):
                centres[i] = members.mean(axis=0)
    return centres


def _hex(colour):
    return ''.join(f'{int(round(c)):02x}' for c in colour)


def measure_look(data):
    """The look of the label in a cart scan, or None when the scan is not a known template."""
    label = _label(data)
    if label is None:
        return None
    px, game_boy = label
    h, w = px.shape[:2]

    candidates = [c for c in (_candidate(px, e) for e in EDGES) if c]
    # A scan that sits off centre leaves some of a side strip past the fixed cut.
    if game_boy:
        candidates = [c for c in candidates if not (c['edge'] in ('left', 'right') and _paper(c['colour']))]
    strong = [c for c in candidates if c['apart'] >= BAND_APART]
    band = max(strong, key=lambda c: c['score']) if strong else None
    borderline = any(abs(c['apart'] - BAND_APART) <= BAND_APART * BORDERLINE for c in candidates)
    if band and any(c is not band and c['score'] >= RIVAL * band['score'] for c in strong):
        borderline = True

    keep = np.ones((h, w), dtype=bool)
    if band:
        across = round((h if band['edge'] in ('top', 'bottom') else w) * band['size'])
        keep_rows = _from(keep, band['edge'])
        keep_rows[:across] = False
    ring = np.zeros((h, w), dtype=bool)
    rh, rw = max(1, round(h * RING)), max(1, round(w * RING))
    ring[:rh], ring[-rh:], ring[:, :rw], ring[:, -rw:] = True, True, True, True
    ring &= keep

    sample = px[keep][::7]
    centres = _kmeans(sample, CLUSTERS)
    ring_px = px[ring][::3]
    nearest = np.argmin(((ring_px[:, None, :] - centres[None]) ** 2).sum(axis=2), axis=1)
    counts = np.bincount(nearest, minlength=CLUSTERS)
    ground = centres[int(np.argmax(counts))]
    share = counts.max() / max(1, len(ring_px))
    # A strip the colour of the ground is the ground running on past something, not a band.
    if band is not None and np.linalg.norm(band['colour'] - ground) < BAND_APART:
        band = None

    return {
        'ground': _hex(ground),
        'band': None if band is None else {
            'edge': band['edge'], 'size': round(band['size'], 3), 'colour': _hex(band['colour'])},
        'sure': bool(share >= GROUND_SHARE and not borderline),
        'by': 'analysis',
    }
