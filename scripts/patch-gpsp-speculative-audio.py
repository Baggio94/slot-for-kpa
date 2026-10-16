#!/usr/bin/env python3
"""Apply Slot's narrowly scoped, idempotent gpSP run-ahead audio sidecar.

Only the pinned source copy in .build/gpsp/source is modified. Upstream
retro_serialize / retro_unserialize and ordinary save-state formats remain
byte-for-byte unchanged. This sidecar is for in-memory speculative rollbacks.
"""
from pathlib import Path
import subprocess
import sys

PIN = "5819380c2ffb0900219d700a382ee68c464ebb99"


def apply_once(path: Path, anchor: str, replacement: str, marker: str) -> None:
    data = path.read_text(encoding="utf-8")
    if marker in data:
        return
    if data.count(anchor) != 1:
        raise RuntimeError(f"{path}: expected exactly one pinned-source anchor")
    path.write_text(data.replace(anchor, replacement, 1), encoding="utf-8")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: patch-gpsp-speculative-audio.py PATH_TO_PINNED_SOURCE")
    src = Path(sys.argv[1]).resolve()
    head = subprocess.check_output(
        ["git", "-C", str(src), "rev-parse", "HEAD"], text=True
    ).strip()
    if head != PIN:
        raise RuntimeError(f"Refusing to patch unreviewed gpSP commit {head}")

    sound_anchor = """static s32 sound_buffer[BUFFER_SIZE];
static u32 sound_buffer_base;"""
    sound_addition = sound_anchor + """

/* Slot Android: transient in-memory mixer snapshot only for run-ahead.
 * Ordinary gpSP savestate semantics (zero the ring on load) are untouched.
 * Restore these samples only after speculative retro_unserialize completed. */
void slot_gpsp_sound_ring_capture(void *dst)
{
  memcpy(dst, sound_buffer, sizeof(sound_buffer));
}

void slot_gpsp_sound_ring_restore(const void *src)
{
  memcpy(sound_buffer, src, sizeof(sound_buffer));
}
"""
    apply_once(src / "sound.c", sound_anchor, sound_addition,
               "void slot_gpsp_sound_ring_capture(void *dst)")

    retro_anchor = "static float audio_samples_accumulator = 0.0f;"
    retro_addition = retro_anchor + """

/* Frontend-private ABI: never part of a RetroArch save-state file.
 * The sound ring is otherwise deliberately omitted from serialisation.
 * Include the fractional sample accumulator to preserve audio cadence. */
void slot_gpsp_sound_ring_capture(void *dst);
void slot_gpsp_sound_ring_restore(const void *src);

size_t slot_gpsp_audio_sidecar_size(void)
{
   return sizeof(audio_samples_accumulator) + BUFFER_SIZE * sizeof(s32);
}

/* restore=false captures, restore=true applies after retro_unserialize.
 * The caller owns and reuses the buffer; no allocation or IO occurs here. */
bool slot_gpsp_audio_sidecar(void *buffer, size_t size, bool restore)
{
   unsigned char *bytes = (unsigned char *)buffer;
   if (!bytes || size != slot_gpsp_audio_sidecar_size())
      return false;

   if (restore)
   {
      memcpy(&audio_samples_accumulator, bytes,
             sizeof(audio_samples_accumulator));
      slot_gpsp_sound_ring_restore(bytes + sizeof(audio_samples_accumulator));
   }
   else
   {
      memcpy(bytes, &audio_samples_accumulator,
             sizeof(audio_samples_accumulator));
      slot_gpsp_sound_ring_capture(bytes + sizeof(audio_samples_accumulator));
   }
   return true;
}
"""
    apply_once(src / "libretro" / "libretro.c", retro_anchor, retro_addition,
               "size_t slot_gpsp_audio_sidecar_size(void)")

    link_anchor = "   global: retro_*;"
    apply_once(src / "link.T", link_anchor,
               "   global: retro_*; slot_gpsp_audio_*;",
               "slot_gpsp_audio_*;")
    print("Applied/verified pinned gpSP speculative-audio sidecar (in RAM only)")


if __name__ == "__main__":
    main()
