#!/usr/bin/env python3
"""mGBA Cable Link: the other player's GBA runs on the other player's BIOS.

Each console emulates both GBAs of a Cable session, so both consoles must run
each GBA the same way, and each GBA's state was made with its owner's BIOS.
slot. tells the core which BIOS the peer GBA uses through the internal option
mgba_link_peer_bios, set before retro_load_game:

  unset / empty  this console's BIOS, as before (both consoles hold the same)
  "hle"          mGBA's built-in BIOS (the other console has none)
  "/path"        that file: the other console's BIOS, received and checked

Idempotent and fail-closed: anchored source must match the pinned mGBA commit
with link-mode.patch applied.
"""
from pathlib import Path
import sys

if len(sys.argv) != 2:
    raise SystemExit("usage: patch-mgba-link-peer-bios.py SOURCE_DIR")
source = Path(sys.argv[1]) / "src/platform/libretro/libretro.c"
data = source.read_text()
MARK = "SLOT_LINK_PEER_BIOS_V1"
if MARK in data:
    print("Already applied: patch-mgba-link-peer-bios.py")
    raise SystemExit(0)

head = """static bool _linkCreatePeer(const struct retro_game_info* game, const char* biosPath) {
"""
load = """#ifdef ENABLE_VFS
\tif (biosPath) {
\t\tstruct VFile* bios = VFileOpen(biosPath, O_RDONLY);
\t\tif (bios) {
\t\t\tpeer->loadBIOS(peer, bios, 0);
\t\t}
\t}
"""
for label, text in (("_linkCreatePeer", head), ("peer BIOS load", load)):
    if data.count(text) != 1:
        raise SystemExit(f"mGBA peer BIOS patch cannot find the audited {label}")

# Read the choice before anything is opened or allocated, so a bad value
# fails the Link boot with nothing to undo.
data = data.replace(head, head + """\t/* SLOT_LINK_PEER_BIOS_V1: the other player's GBA keeps its own BIOS. */
\tstruct retro_variable peerBiosVar = { .key = "mgba_link_peer_bios", .value = NULL };
\tif (environCallback(RETRO_ENVIRONMENT_GET_VARIABLE, &peerBiosVar) &&
\t\tpeerBiosVar.value && peerBiosVar.value[0]) {
\t\tif (strcmp(peerBiosVar.value, "hle") == 0) {
\t\t\tbiosPath = NULL;
\t\t} else if (peerBiosVar.value[0] == '/' && strlen(peerBiosVar.value) < 1024) {
\t\t\tbiosPath = peerBiosVar.value;
\t\t} else {
\t\t\treturn false;
\t\t}
\t}
""", 1)
source.write_text(data)
assert source.read_text().count(MARK) == 1
print("Applied: patch-mgba-link-peer-bios.py")
