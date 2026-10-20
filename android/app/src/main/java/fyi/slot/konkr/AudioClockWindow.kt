package fyi.slot.konkr

/**
 * Beta12 diagnostic-only reconciliation between the committed GBA timeline
 * (GL-owned libretro core) and Android's audio playback writer.
 *
 * Counters are cumulative atomics read on the existing audio worker only
 * every ~5s. Do not pace the game, mutate PCM, touch AudioTrack or infer
 * input-to-audio latency from these values.
 */
internal object AudioClockWindow {
    const val COUNTERS = 7

    data class Delta(
        val callbacks: Long,
        val committedFrames: Long,
        val pcmProducedShorts: Long,
        val pcmQueuedShorts: Long,
        val emptyPcmFrames: Long,
        val noStepCallbacks: Long,
        val overdueCallbacks: Long,
        val spanMs: Long,
        val sampleRate: Int,
    ) {
        val frameRate: Double get() = committedFrames * 1000.0 / spanMs
        val callbackRate: Double get() = callbacks * 1000.0 / spanMs
        val producedPcmMs: Double get() = pcmProducedShorts * 500.0 / sampleRate
        val queuedPcmMs: Double get() = pcmQueuedShorts * 500.0 / sampleRate
        val producerDeficitMs: Double get() = spanMs - producedPcmMs
    }

    fun between(before: LongArray, after: LongArray, spanMs: Long, sampleRate: Int): Delta? {
        if (before.size != COUNTERS || after.size != COUNTERS ||
            spanMs <= 0 || sampleRate <= 0) return null
        val delta = LongArray(COUNTERS)
        for (index in 0 until COUNTERS) {
            if (after[index] < before[index]) return null
            delta[index] = after[index] - before[index]
        }
        return Delta(delta[0], delta[1], delta[2], delta[3],
            delta[4], delta[5], delta[6], spanMs, sampleRate)
    }
}
