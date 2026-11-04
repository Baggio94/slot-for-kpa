#!/usr/bin/env python3
"""Add the TWB64 028 "Game.com Ver." palette to mGBA's Game Boy presets.

slot.'s default Game Boy palette since 1.3.5. Gambatte ships it in its TWB64
Pack 1; mGBA does not, so it is appended to mGBA's preset list under the name
slot. passes in mgba_gb_colors. Colours are TWB64's own (gambatte-libretro
gbcpalettes.h, twb64_028_game_com_ver): one 4-shade ramp for BG, OBJ0, OBJ1.

Appending keeps every existing preset's index and name. Save states and
libretro entry points stay unchanged.
"""
from pathlib import Path
import sys

if len(sys.argv) != 2:
    raise SystemExit("usage: patch-mgba-twb64.py SOURCE_DIR")
source = Path(sys.argv[1]) / "src/gb/overrides.c"
data = source.read_text()
name = "TWB64 028 Game.com Ver."
if name not in data:
    anchor = '\t{ "SGB 4-H", SGB_PAL(4H), },\n};\n'
    if data.count(anchor) != 1:
        raise SystemExit("mGBA TWB64 patch cannot find the audited preset list end")
    ramp = "0xFFA7BF6B, 0xFF6F8F4F, 0xFF0F4F2F, 0xFF000000"
    entry = f'\t{{ "{name}", {{ {ramp}, {ramp}, {ramp} }}, }},\n'
    data = data.replace(anchor, '\t{ "SGB 4-H", SGB_PAL(4H), },\n' + entry + "};\n", 1)
    source.write_text(data)
assert source.read_text().count(f'"{name}"') == 1
print("Applied: patch-mgba-twb64.py")
