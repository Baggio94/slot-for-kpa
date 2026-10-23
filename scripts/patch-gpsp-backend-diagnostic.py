#!/usr/bin/env python3
"""Attach a read-only gpSP dynarec/interpreter diagnostic to the pinned core.

Do NOT patch JIT memory, RWX permissions, compiler flags or emulated timing.
Expose the mode gpSP already selected. The frontend logs it off the hot path.
"""
import sys
from pathlib import Path

if len(sys.argv) != 2:
    raise SystemExit("usage: patch-gpsp-backend-diagnostic.py GPSP_SOURCE")
root = Path(sys.argv[1])
source = root / "libretro/libretro.c"
text = source.read_text()
symbol = "slot_gpsp_backend_is_interpreter"
anchor = "void retro_get_system_av_info(struct retro_system_av_info* info)"
if symbol not in text:
    if text.count(anchor) != 1:
        raise SystemExit("gpSP diagnostic injection anchor mismatch")
    addition = """/* KPA diagnostic-only ABI: 1=interpreter, 0=ARM64 dynarec.
 * gpSP chooses this backend itself. Do not change its option or JIT policy.
 */
int slot_gpsp_backend_is_interpreter(void)
{
#if defined(HAVE_DYNAREC)
    return dynarec_enable ? 0 : 1;
#else
    return 1;
#endif
}

"""
    source.write_text(text.replace(anchor, addition + anchor))
else:
    assert text.count(symbol) == 1, "duplicate gpSP backend getter"

link = root / "link.T"
mapping = link.read_text()
name = "slot_gpsp_backend_is_interpreter;"
if name not in mapping:
    anchor = "   global: retro_*; slot_gpsp_audio_*;"
    if mapping.count(anchor) != 1:
        raise SystemExit("gpSP library export script changed")
    link.write_text(mapping.replace(anchor, anchor + " " + name))
print("Verified gpSP read-only dynarec/interpreter diagnostic ABI")
