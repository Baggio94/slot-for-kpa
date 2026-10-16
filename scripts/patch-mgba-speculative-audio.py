#!/usr/bin/env python3
"""Pinned mGBA-only speculative-audio sidecar for slot. KONKR.

Keeps ordinary libretro retro_serialize/retro_unserialize and RetroArch state
files exactly as upstream. Only the transient in-memory rollback path uses
these sidecar exports. No changes to mGBA's frame/audio pacing algorithms.
"""
from pathlib import Path
import subprocess
import sys

PIN = "7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6"


def replace_once(file: Path, old: str, new: str, marker: str) -> None:
    data = file.read_text(encoding="utf-8")
    if marker in data:
        return
    if data.count(old) != 1:
        raise RuntimeError(f"Expected one pinned-source anchor in {file}: {old[:70]}")
    file.write_text(data.replace(old, new, 1), encoding="utf-8")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: patch-mgba-speculative-audio.py PINNED_SOURCE_DIRECTORY")
    root = Path(sys.argv[1]).resolve()
    head = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
    ).strip()
    if head != PIN:
        raise RuntimeError(f"Refusing to patch unreviewed mGBA revision {head}")

    file = root / "src/platform/libretro/libretro.c"
    anchor = "static int32_t audioLowPassRightPrev = 0;"
    addition = anchor + """

/* slot. Android: transient output-audio snapshot (NOT a savestate change).
 * mGBA's GBA/GB APU state is serialized, but its libretro converter, optional
 * low-pass history and queued output samples are not. A single-instance
 * run-ahead rollback must restore these too, after retro_unserialize. */
/* Derived PSG channel samples are omitted by mGBA's normal savestate
 * serializer. On speculative rollback, preserve their *exact* values,
 * rather than leaving samples synthesized by the speculative frame.
 * In particular, channel 4 noise changes timbre without any dropout.
 *
 * mGBA's GBAudioPSGSerialize() is for *persistent* and backward-compatible
 * savestates: it intentionally does not roundtrip every derived PSG field
 * (for example ch1.sweep.step). On per-frame run-ahead, we must restore the
 * exact transient values of all four *plain value* channel structures.
 * Never snapshot the parent GBAudio: it contains pointers and timing events.
 */
static struct GBAudio* slot_mgba_psg(void) {
    if (!core) {
        return NULL;
    }
#ifdef M_CORE_GBA
    if (core->platform(core) == mPLATFORM_GBA) {
        struct GBA* gba = core->board;
        return &gba->audio.psg;
    }
#endif
#ifdef M_CORE_GB
    if (core->platform(core) == mPLATFORM_GB) {
        struct GB* gb = core->board;
        return &gb->audio;
    }
#endif
    return NULL;
}

struct SlotMgbaAudioSnapshot {
    struct LibretroAudioConverter converter;
    int32_t lowPassLeft;
    int32_t lowPassRight;
    struct GBAudioSquareChannel ch1;
    struct GBAudioSquareChannel ch2;
    struct GBAudioWaveChannel ch3;
    struct GBAudioNoiseChannel ch4;
    size_t frames;
};

size_t slot_mgba_audio_sidecar_size(void) {
    if (!core) {
        return 0;
    }
    struct mAudioBuffer* queue = core->getAudioBuffer(core);
    if (!queue || queue->channels != 2 || !slot_mgba_psg()) {
        return 0;
    }
    size_t cap = mAudioBufferCapacity(queue);
    if (!cap || cap > 131072) {
        return 0;
    }
    return sizeof(struct SlotMgbaAudioSnapshot) + cap * 2 * sizeof(int16_t);
}

/* restore=0 captures; restore=1 applies after the normal libretro rollback.
 * The caller reuses a byte buffer; no heap allocations or disk IO per frame.
 * Only queued live samples are saved, not unused capacity/ring pointers. */
bool slot_mgba_audio_sidecar(void* buffer, size_t size, bool restore) {
    if (!core || !buffer || size != slot_mgba_audio_sidecar_size()) {
        return false;
    }
    struct mAudioBuffer* queue = core->getAudioBuffer(core);
    struct GBAudio* psg = slot_mgba_psg();
    if (!queue || !psg) {
        return false;
    }
    struct SlotMgbaAudioSnapshot snapshot;
    memset(&snapshot, 0, sizeof(snapshot));
    unsigned char* bytes = (unsigned char*) buffer;
    int16_t* pcm = (int16_t*) (bytes + sizeof(snapshot));
    if (restore) {
        memcpy(&snapshot, bytes, sizeof(snapshot));
        if (snapshot.frames > mAudioBufferCapacity(queue)) {
            return false;
        }
        audioConverter = snapshot.converter;
        audioLowPassLeftPrev = snapshot.lowPassLeft;
        audioLowPassRightPrev = snapshot.lowPassRight;
        /* Apply AFTER retro_unserialize, because normal PSG restore can
         * normalize the hardware-visible channel fields as a savestate
         * compatibility measure. The speculative timeline needs the exact
         * pre-frame values (including sample and sweep countdown).
         * These four structures are pure values, never heap pointers.
         */
        psg->ch1 = snapshot.ch1;
        psg->ch2 = snapshot.ch2;
        psg->ch3 = snapshot.ch3;
        psg->ch4 = snapshot.ch4;
        mAudioBufferClear(queue);
        return mAudioBufferWrite(queue, pcm, snapshot.frames) == snapshot.frames;
    }

    snapshot.converter = audioConverter;
    snapshot.lowPassLeft = audioLowPassLeftPrev;
    snapshot.lowPassRight = audioLowPassRightPrev;
    snapshot.ch1 = psg->ch1;
    snapshot.ch2 = psg->ch2;
    snapshot.ch3 = psg->ch3;
    snapshot.ch4 = psg->ch4;
    snapshot.frames = mAudioBufferAvailable(queue);
    if (snapshot.frames > mAudioBufferCapacity(queue)) {
        return false;
    }
    if (mAudioBufferDump(queue, pcm, snapshot.frames, 0) != snapshot.frames) {
        return false;
    }
    memcpy(bytes, &snapshot, sizeof(snapshot));
    return true;
}
"""
    replace_once(file, anchor, addition, "size_t slot_mgba_audio_sidecar_size(void)")

    link = root / "src/platform/libretro/link.T"
    link_anchor = "   global: retro_*;"
    replace_once(link, link_anchor,
                 "   global: retro_*; slot_mgba_audio_*;",
                 "slot_mgba_audio_*;")
    print("Applied or verified pinned mGBA in-memory audio rollback ABI")


if __name__ == "__main__":
    main()
