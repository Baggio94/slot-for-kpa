#!/usr/bin/env python3
"""Add KPA-only export that sets the solar sensor light (Boktai 1/2/3).

mGBA's libretro core only takes sunlight from its mgba_solar_sensor_level
option (levels 0..10, at most 205 of 255) or from the frontend's light
sensor. The option is reread only when the game samples its sensor right
after a changed-options frame, which a resumed game can miss: the gauge
then stays stuck. slot_mgba_set_sun(level) hands the cartridge its light
directly, 0..255, on every sample; -1 gives the choice back to the option.

The save-state format and every libretro entry point stay unchanged.
"""
from pathlib import Path
import sys

if len(sys.argv) != 2:
    raise SystemExit("usage: patch-mgba-sun.py SOURCE_DIR")
source = Path(sys.argv[1]) / "src/platform/libretro/libretro.c"
data = source.read_text()
symbol = "slot_mgba_set_sun"
if symbol not in data:
    edits = [
        ("static bool luxSensorUsed;\n",
         "static bool luxSensorUsed;\n"
         "/* KPA: light chosen by the frontend, -1 when the option decides. */\n"
         "static int slotSunLevel = -1;\n"),
        # The last statement of _updateLux: the chosen light wins over both
        # the option's level and the frontend light sensor.
        ("\t\tluxLevel = 0x16;\n\t\tif (luxLevelIndex > 0) {\n"
         "\t\t\tluxLevel += GBA_LUX_LEVELS[luxLevelIndex - 1];\n\t\t}\n\t}\n\n"
         "\tenvVarsUpdated = false;\n",
         "\t\tluxLevel = 0x16;\n\t\tif (luxLevelIndex > 0) {\n"
         "\t\t\tluxLevel += GBA_LUX_LEVELS[luxLevelIndex - 1];\n\t\t}\n\t}\n"
         "\tif (slotSunLevel >= 0) {\n\t\tluxLevel = slotSunLevel;\n\t}\n\n"
         "\tenvVarsUpdated = false;\n"),
        ("void* retro_get_memory_data(unsigned id) {",
         "/* KPA: the solar sensor's light, 0..255, read by the game on its next\n"
         " * sample. -1 returns to mgba_solar_sensor_level. */\n"
         "void slot_mgba_set_sun(int level) {\n"
         "\tslotSunLevel = level < 0 ? -1 : level > 255 ? 255 : level;\n"
         "}\n\n"
         "void* retro_get_memory_data(unsigned id) {"),
    ]
    for anchor, replacement in edits:
        if data.count(anchor) != 1:
            raise SystemExit("mGBA libretro anchor changed; refusing source mutation")
        data = data.replace(anchor, replacement)
    source.write_text(data)
# mGBA's libretro version script exports only retro_* and symbols named here.
link = Path(sys.argv[1]) / "src/platform/libretro/link.T"
script = link.read_text()
export_name = symbol + ";"
if export_name not in script:
    expected = "   global: retro_*; slot_mgba_audio_*;"
    if script.count(expected) != 1:
        raise SystemExit("mGBA libretro version script changed; refuse unverified export")
    link.write_text(script.replace(expected, expected + " " + export_name))
print("Applied: KPA solar sensor light export")
