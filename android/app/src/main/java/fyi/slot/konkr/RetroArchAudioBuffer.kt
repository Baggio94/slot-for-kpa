package fyi.slot.konkr

/**
 * RetroArch's OpenSL ES output buffer at its default 128 ms audio latency
 * (audio/drivers/opensl.c): blocks of next_pow2(32 * latency) bytes, as many
 * as fill the latency at the output rate, never fewer than two. Dynamic rate
 * control keeps it half full, so sound plays about 64 ms after the core made
 * it, as in RetroArch.
 */
internal object RetroArchAudioBuffer {
    /** Solo play always resamples to this rate (ra_sync::OUTPUT_RATE). */
    const val OUTPUT_RATE = 48_000
    const val LATENCY_MS = 128

    /** Bytes of interleaved PCM16 stereo at [rate]. */
    fun bytes(rate: Int, latencyMs: Int = LATENCY_MS): Int {
        var block = 1L
        while (block < 32L * latencyMs) block = block shl 1
        val fill = (latencyMs.toLong() * 4L * rate + 500L) / 1000L
        val blocks = ((fill + block / 2L) / block).coerceAtLeast(2L)
        return (blocks * block).toInt()
    }
}
