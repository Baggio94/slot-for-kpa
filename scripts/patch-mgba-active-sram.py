#!/usr/bin/env python3
"""Add KPA-only read-only mGBA active battery clone export.

The stock retro_get_memory_data(RETRO_MEMORY_SAVE_RAM) points to libretro's
backing buffer. After a savestate restore, mGBA may temporarily run its
cart save-data from GBASavedataMask's *different* active memory. A clone of
core->savedataClone() reads the actual save-data currently seen by the game.

This export does not unmask, write memory, alter save-state format, or change
libretro API behavior; it only copies on explicit battery save/export.
"""
from pathlib import Path
import sys

if len(sys.argv) != 2:
    raise SystemExit("usage: patch-mgba-active-sram.py SOURCE_DIR")
source = Path(sys.argv[1]) / "src/platform/libretro/libretro.c"
data = source.read_text()
anchor = "void* retro_get_memory_data(unsigned id) {"
symbol = "slot_mgba_clone_active_save_ram"
if symbol in data:
    assert data.count(symbol) == 1, "duplicate mGBA save helper"
    print("Already applied: mGBA active SRAM clone export")
    raise SystemExit(0)
if data.count(anchor) != 1:
    raise SystemExit("mGBA libretro anchor changed; refusing source mutation")
export = """/* KPA-only read-only snapshot of the ACTIVE cartridge battery data.
 * retro_get_memory_data(SAVE_RAM) may point to a stale libretro mirror while
 * GBASavedataMask is active after retro_unserialize(). Never change the
 * persistent libretro save-state protocol to fix this.
 *
 * Return true with written=0 for a cartridge without detected battery.
 * Return false for an invalid buffer or failed nonempty clone. The frontend
 * must abort battery export on failure; no stale-pointer fallback.
 */
bool slot_mgba_clone_active_save_ram(void* out, size_t capacity, size_t* written) {
#ifdef M_CORE_GBA
    if (!written) {
        return false;
    }
    *written = 0;
    if (!core) {
        return false;
    }
    if (core->platform(core) != mPLATFORM_GBA) {
        *written = (size_t)-1; /* GB/GBC: preserve standard libretro battery */
        return true;
    }
    struct GBA* gba = core->board;
    if (!gba || !core->savedataClone) {
        return false;
    }
    const size_t expected = GBASavedataSize(&gba->memory.savedata);
    if (!expected) {
        return true; /* no currently detected SRAM/EEPROM/Flash */
    }
    if (!out || expected > GBA_SIZE_FLASH1M || capacity < expected) {
        return false;
    }
    void* cloned = NULL;
    const size_t size = core->savedataClone(core, &cloned);
    if (!cloned || size != expected) {
        free(cloned);
        return false;
    }
    memcpy(out, cloned, size);
    free(cloned);
    *written = size;
    return true;
#else
    (void)out;
    (void)capacity;
    if (written) *written = 0;
    return false;
#endif
}

"""
source.write_text(data.replace(anchor, export + anchor))
print("Applied: KPA read-only active SRAM export (F5)")
