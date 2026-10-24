#!/usr/bin/env python3
"""Draw the Game Boy pak board in the style of crates/slot-ui/assets/board.svg.

The layout is traced from assets/gb_board.png (312x337 px): chips, holes,
battery, vias and traces sit where the photographed board has them. Colours,
pins, vias and stroke widths are the GBA board's (one GBA board unit = 1.55 px).

Writes, into crates/slot-ui/assets/:
  gb_board.svg   the board with both big-chip footprints empty, holes cut out
  gb_shell.svg   the notched (DMG) back shell under it, in placeholder colours
  gbc_shell.svg  the rounded (GBC) back shell
  gb_chip.svg    the big chip the core picker hops between the footprints
The shells use board.svg's placeholders (#ff00ff plastic, #800080 floor,
#400040 deep), replaced with the game's colour at runtime.
"""
import pathlib

OUT = pathlib.Path(__file__).resolve().parents[2] / "crates/slot-ui/assets"
U = 1.55
GREEN, EDGE, TRACE = "#3a9a3c", "#2e7f30", "#6cc463"
VIA, VIA_IN = "#74c466", "#1d4a1d"
CHIP, DOT, PIN, GOLD, SILK = "#1a1a1a", "#333333", "#b3b3b8", "#d4a93c", "#eef5e6"
METAL, METAL_EDGE, METAL_RING, LIGHT = "#bfc0c6", "#8c8d93", "#a5a6ac", "#cfd0d4"
YELLOW, YELLOW_EDGE, YELLOW_IN = "#f2c21b", "#c99a0c", "#dcae12"
PLASTIC, FLOOR, DEEP = "#ff00ff", "#800080", "#400040"

PCB = [(90, 7), (221, 7), (221, 12), (261, 12), (261, 30), (287, 30), (287, 89), (292, 89),
       (292, 104), (287, 104), (287, 210), (292, 210), (292, 225), (287, 225), (287, 277),
       (283, 278), (283, 317), (278, 322), (33, 322), (28, 317), (28, 278), (24, 277),
       (24, 225), (19, 225), (19, 210), (24, 210), (24, 104), (19, 104), (19, 89), (24, 89),
       (24, 13), (90, 12)]
HOLES = [(130.5, 32.5, 8), (156, 112, 6.5), (155.5, 231, 19)]
# Big-chip footprints: centres and chip size, as slot-ui's board.rs has them.
FOOTPRINTS = [(99, 213.5), (219, 213.5)]
CHIP_W, CHIP_H = 70, 91
FINGERS = [38 + i * (273 - 38) / 26 for i in range(27)]
VIAS = [(89, 20), (115, 20), (168, 20), (192, 20), (218, 20), (244, 21), (280, 36), (280, 56), (76, 66), (83, 80), (280, 80), (161, 84), (50, 85), (113, 86), (153, 86), (45, 89), (50, 93), (122, 96), (87, 98), (110, 98), (134, 98), (121, 102), (141, 102), (131, 106), (280, 107), (64, 110), (122, 110), (48, 112), (129, 114), (59, 116), (136, 117), (280, 118), (63, 121), (132, 123), (59, 126), (126, 126), (155, 127), (161, 127), (139, 128), (132, 130), (126, 134), (142, 134), (60, 136), (62, 142), (134, 142), (64, 149), (92, 160), (188, 161), (62, 163), (162, 174), (181, 174), (56, 175), (280, 177), (59, 184), (260, 194), (280, 194), (59, 208), (280, 220), (277, 250), (51, 251), (59, 251), (270, 260), (277, 260), (33, 261), (175, 263), (190, 263), (43, 264), (55, 264), (130, 265), (152, 265), (71, 266), (77, 266), (33, 269), (246, 269), (259, 270), (253, 271), (43, 272), (55, 272)]
TRACES = [
    "M89 20 H50 V81", "M83 24 V76", "M60 56 V93 H67", "M45 89 H34 V163 H56",
    "M163 21 Q154 26 154 34 V80", "M161 58 H168", "M161 60 V80", "M172 21 H188", "M222 21 H240",
    "M218 25 V37 H250 V21", "M34 173 H55 M34 173 V208 H56", "M34 218 H68 M38 222 H66 M38 222 V250 H44",
    "M60 183 H66", "M60 190 H66", "M60 197 H66", "M58 225 H66", "M62 262 H66",
    "M129 181 H182", "M129 187 H182", "M129 193 H182",
    "M257 176 H275", "M264 192 H275", "M258 200 H280 V160", "M257 210 H270 V205 H257", "M257 220 H275",
    "M258 248 H272", "M258 254 L275 258", "M190 260 V273 H157 M190 270 H160", "M278 160 V175",
    "M84 157 V166", "M91 157 V166", "M98 157 V166", "M105 157 V166",
    "M64 110 H66", "M59 116 H66", "M63 121 H66", "M59 126 H66", "M60 136 H66", "M62 142 H66",
    "M126 126 H124", "M132 123 H124", "M139 128 H134", "M141 102 L126 112",
] + [f"M{x} 266 V276.5" for x in (72, 78, 90, 98, 106, 114, 122)] \
  + [f"M{x} 262 V276.5" for x in (202, 211, 220, 229, 238)]
SHELLS = {
    "gb": "M 9.8 0 L 208.9 0 A 4.2 4 0 0 1 213.1 4 L 213.1 10.7 A 0.8 0.8 0 0 0 213.9 11.5 L 228.7 11.5 A 4.8 5 0 0 1 233.5 16.5 L 233.5 255.6 A 4.3 3.4 0 0 1 229.2 259 L 10.8 259 A 4.3 3.4 0 0 1 6.5 255.6 L 6.5 2.4 A 3.3 2.4 0 0 1 9.8 0 Z",
    "gbc": "M 24.5 0 L 215.5 0 A 18 18 0 0 1 233.5 18 L 233.5 255.6 A 4.3 3.4 0 0 1 229.2 259 L 10.8 259 A 4.3 3.4 0 0 1 6.5 255.6 L 6.5 18 A 18 18 0 0 1 24.5 0 Z",
}


def n(v):
    return f"{v:.2f}".rstrip("0").rstrip(".")


def svg(w, h, body, defs=""):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">\n'
            + (f"  <defs>{defs}</defs>\n" if defs else "") + "".join(f"  {e}\n" for e in body) + "</svg>\n")


def circle_path(x, y, r):
    return f"M{n(x - r)} {n(y)} a{n(r)} {n(r)} 0 1 0 {n(2 * r)} 0 a{n(r)} {n(r)} 0 1 0 {n(-2 * r)} 0 Z"


def outline(pts, dx=0.0, dy=0.0):
    return "M" + " L".join(f"{n(x + dx)} {n(y + dy)}" for x, y in pts) + " Z"


def silk_box(x0, y0, x1, y1, r=3.0):
    return (f'<rect x="{n(x0)}" y="{n(y0)}" width="{n(x1 - x0)}" height="{n(y1 - y0)}" rx="{n(r)}" '
            f'fill="none" stroke="{SILK}" stroke-width="{n(.7 * U)}"/>')


def chip_pins(x0, y0, metal):
    pitch, count, height, length = 5.8, 15, 3.2, 8
    top = y0 + CHIP_H / 2 - (count - 1) * pitch / 2
    pins = []
    for i in range(count):
        y = top + i * pitch - height / 2
        for x in (x0 + 4, x0 + CHIP_W - 4 - length):
            pins.append(f'<rect x="{n(x)}" y="{n(y)}" width="{length}" height="{n(height)}" fill="{metal}"/>')
    return pins


def board():
    holes = "".join(" " + circle_path(x, y, r) for x, y, r in HOLES)
    s = [f'<path d="{outline(PCB, 2.3, 3)}{holes}" fill-rule="evenodd" fill="#000" opacity=".45" filter="url(#softer)"/>',
         f'<path d="{outline(PCB)}{holes}" fill-rule="evenodd" fill="{GREEN}" stroke="{EDGE}" stroke-width="{n(.6 * U)}"/>']
    s.append(f'<g fill="none" stroke="{TRACE}" stroke-width="{n(.9 * U)}" stroke-linejoin="round" '
             f'stroke-linecap="round" opacity=".6">' + "".join(f'<path d="{d}"/>' for d in TRACES) + "</g>")
    vias = list(VIAS) + [(x, 279.5) for x in FINGERS]
    s.append(f'<g fill="{VIA}">' + "".join(f'<circle cx="{n(x)}" cy="{n(y)}" r="{n(1.5 * U)}"/>' for x, y in vias) + "</g>")
    s.append(f'<g fill="{VIA_IN}">' + "".join(f'<circle cx="{n(x)}" cy="{n(y)}" r="{n(.6 * U)}"/>' for x, y in vias) + "</g>")
    s.append(f'<g fill="{GOLD}">' + "".join(f'<rect x="{n(x - 3.2)}" y="284" width="6.4" height="32" rx="{n(.8 * U)}"/>' for x in FINGERS) + "</g>")
    for x, y, r in HOLES:
        s.append(f'<circle cx="{n(x)}" cy="{n(y)}" r="{n(r)}" fill="none" stroke="{SILK if r > 10 else EDGE}" stroke-width="{n(.6 * U)}"/>')
    # Crystal: can, two pads, bent leads.
    s.append(f'<path d="M76 50 C82 50 82 44.5 89 44.5 M76 52 C82 52 82 57.5 89 57.5" fill="none" stroke="{PIN}" stroke-width="1.8"/>')
    s.append(f'<rect x="56" y="47.5" width="22" height="7" rx="2" fill="{METAL}" stroke="{METAL_EDGE}" stroke-width="{n(.6 * U)}"/>')
    for y in (41.5, 54.5):
        s.append(f'<rect x="89" y="{y}" width="20" height="6" rx="1.5" fill="{METAL}" stroke="{METAL_EDGE}" stroke-width="{n(.6 * U)}"/>')
    # Eight-pin chip under the crystal.
    s.append(silk_box(114.5, 46, 148.5, 90.5))
    for x in (119, 126.5, 134, 141.5):
        s.append(f'<rect x="{x}" y="49.5" width="4" height="8" fill="{PIN}"/><rect x="{x}" y="79.5" width="4" height="8" fill="{PIN}"/>')
    s.append(f'<rect x="117" y="57" width="29" height="23" rx="{n(U)}" fill="{CHIP}"/><circle cx="120.5" cy="76.5" r="{n(U)}" fill="{DOT}"/>')
    # The square chip the game title is printed on (board.rs PAK_MARK_*).
    s.append(silk_box(64, 98, 126, 160, 3.5))
    for i in range(5):
        o = 79 + i * 7
        s.append(f'<rect x="{o}" y="101" width="4" height="7" fill="{PIN}"/><rect x="{o}" y="150" width="4" height="7" fill="{PIN}"/>')
        s.append(f'<rect x="67" y="{o + 34}" width="7" height="4" fill="{PIN}"/><rect x="116" y="{o + 34}" width="7" height="4" fill="{PIN}"/>')
    s.append(f'<rect x="74" y="108" width="42" height="42" rx="{n(U)}" fill="{CHIP}"/><circle cx="78" cy="146" r="{n(1.1 * U)}" fill="{DOT}"/>')
    # Battery in its yellow holder.
    s.append(f'<g transform="rotate(-38 179 52)"><rect x="166" y="46" width="26" height="12" rx="1" fill="{LIGHT}" stroke="{METAL_EDGE}" stroke-width="{n(.6 * U)}"/><rect x="183" y="49.5" width="4" height="5" fill="{GREEN}"/></g>')
    s.append(f'<circle cx="223" cy="96" r="50" fill="{YELLOW}" stroke="{YELLOW_EDGE}" stroke-width="{n(.9 * U)}"/>')
    s.append(f'<circle cx="224" cy="96.5" r="38" fill="{YELLOW_IN}"/>')
    s.append(f'<circle cx="224" cy="96.5" r="35" fill="{METAL}" stroke="{METAL_EDGE}" stroke-width="{n(.9 * U)}"/>')
    s.append(f'<g transform="rotate(41 207 91)"><rect x="207" y="85" width="88" height="12" rx="2.5" fill="{LIGHT}" stroke="{METAL_EDGE}" stroke-width="{n(.6 * U)}"/><rect x="211" y="88.5" width="68" height="5" rx="2.5" fill="none" stroke="{METAL_RING}" stroke-width="{n(.6 * U)}"/></g>')
    for x, y in [(261, 38), (44, 228)]:
        s.append(f'<rect x="{x}" y="{y}" width="17" height="17" rx="1.5" fill="{GOLD}"/><rect x="{x + 6}" y="{y + 6}" width="5" height="5" fill="#1a3d1a"/>')
    # Empty big-chip footprints: outline and gold pads.
    for cx, cy in FOOTPRINTS:
        s.append(silk_box(cx - CHIP_W / 2 + 1, cy - CHIP_H / 2 - 3, cx + CHIP_W / 2 - 1, cy + CHIP_H / 2 + 3, 3.5))
        s += chip_pins(cx - CHIP_W / 2, cy - CHIP_H / 2, GOLD)
    defs = f'<filter id="softer" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="{n(1.6 * U)}"/></filter>'
    return svg(312, 337, s, defs)


def shell(kind):
    path = SHELLS[kind]
    inner = "translate(156 168.5) scale(1.24 1.255) translate(-120 -129.5)"
    s = [f'<path transform="scale(1.3 1.3012)" d="{path}" fill="{PLASTIC}"/>',
         f'<path transform="{inner}" d="{path}" fill="{FLOOR}"/>',
         f'<path transform="{inner}" d="{path}" fill="url(#wallShade)"/>']
    # The pegs and screw post the board's holes sit on.
    for x, y, r in HOLES:
        if r > 10:
            s.append(f'<circle cx="{n(x)}" cy="{n(y - 1.5)}" r="12" fill="{PLASTIC}" stroke="{DEEP}" stroke-width="{n(.8 * U)}"/>')
            s.append(f'<circle cx="{n(x)}" cy="{n(y - 1.5)}" r="4.6" fill="{DEEP}"/>')
        else:
            s.append(f'<circle cx="{n(x)}" cy="{n(y - 1)}" r="{n(r - 2.2)}" fill="{PLASTIC}" stroke="{DEEP}" stroke-width="{n(.6 * U)}"/>')
    defs = ('<linearGradient id="wallShade" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#000" '
            'stop-opacity=".6"/><stop offset=".08" stop-color="#000" stop-opacity="0"/></linearGradient>')
    return svg(312, 337, s, defs)


def chip():
    s = chip_pins(0, 0, PIN)
    s.append(f'<rect x="12" y="1" width="{CHIP_W - 24}" height="{CHIP_H - 2}" rx="{n(U)}" fill="{CHIP}"/>')
    s.append(f'<circle cx="17.5" cy="{CHIP_H - 7}" r="{n(1.3 * U)}" fill="{DOT}"/>')
    return svg(CHIP_W, CHIP_H, s)


if __name__ == "__main__":
    (OUT / "gb_board.svg").write_text(board())
    (OUT / "gb_shell.svg").write_text(shell("gb"))
    (OUT / "gbc_shell.svg").write_text(shell("gbc"))
    (OUT / "gb_chip.svg").write_text(chip())
