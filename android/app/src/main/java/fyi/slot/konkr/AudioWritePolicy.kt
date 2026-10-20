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
        start: Int = 0,
        length: Int = samples.size - start,
        writer: (offset: Int, remaining: Int) -> Int,
    ): Int {
        require(start >= 0 && length >= 0 && length <= samples.size - start)
        var offset = start
        val end = start + length
        var attempts = 0
        while (offset < end && attempts < 64) {
            val left = end - offset
            val count = writer(offset, left)
            if (count <= 0 || count > left) break
            offset += count
            attempts++
        }
        return offset - start
    }
}
