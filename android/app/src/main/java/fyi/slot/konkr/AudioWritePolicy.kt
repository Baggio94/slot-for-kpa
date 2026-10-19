package fyi.slot.konkr

/**
 * Handles Android AudioTrack's documented short-write outcomes without
 * discarding the unwritten tail of an interleaved PCM16 stereo chunk.
 * The writer is inlined to avoid a hot-path lambda allocation.
 *
 * Never alters the samples, their rate, the queue or the playback buffer.
 * A bounded attempt count prevents pathological drivers from looping forever.
 */
internal object AudioWritePolicy {
    inline fun writeComplete(
        samples: ShortArray,
        writer: (offset: Int, remaining: Int) -> Int,
    ): Int {
        var offset = 0
        var attempts = 0
        while (offset < samples.size && attempts < 64) {
            val left = samples.size - offset
            val count = writer(offset, left)
            if (count <= 0 || count > left) break
            offset += count
            attempts++
        }
        return offset
    }
}
