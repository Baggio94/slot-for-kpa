package fyi.slot.konkr

/**
 * Diagnostic-only PCM FIFO loss between two ~5s audio probes.
 *
 * Counters are cumulative atomics owned by the GL thread and read on the
 * existing audio worker only. Do not pace the game, mutate PCM or touch
 * AudioTrack from these values.
 */
internal object AudioClockWindow {
    const val COUNTERS = 2

    data class Delta(
        val fifoOverflowEvents: Long,
        val fifoDroppedShorts: Long,
    )

    fun between(before: LongArray, after: LongArray): Delta? {
        if (before.size != COUNTERS || after.size != COUNTERS) return null
        val overflow = after[0] - before[0]
        val dropped = after[1] - before[1]
        if (overflow < 0 || dropped < 0) return null
        return Delta(overflow, dropped)
    }
}
