package fyi.slot.konkr

/**
 * Beta10: GL-thread-only correlation between Choreographer/UI vsync deliveries,
 * real GL callback gaps and the previous committed nativeDrawFrame duration.
 *
 * This is diagnostic evidence, NOT a display-present timestamp. These counters
 * cannot establish photon latency, dropped display frames or causal ownership.
 * No synchronization, I/O, allocations or EGL operations on the GL hot path.
 */
internal class FramePacingCorrelation {
    private var previousNativeUs = 0L
    private var lastVsyncSequence = 0L
    private var priorVsyncEnabled = false

    private var slowGaps = 0L
    private var slowGapsAfterExpensiveNative = 0L
    private var slowGapsAfterNormalNative = 0L
    private var slowGapsWithExpensiveCurrentNative = 0L
    private var glCallbacksWithRepeatedVsync = 0L
    private var vsyncCallbacksWithoutGl = 0L

    data class Snapshot(
        val slowGaps: Long,
        val slowGapsAfterExpensiveNative: Long,
        val slowGapsAfterNormalNative: Long,
        val slowGapsWithExpensiveCurrentNative: Long,
        val glCallbacksWithRepeatedVsync: Long,
        val vsyncCallbacksWithoutGl: Long,
    )

    /**
     * Record one real GL draw callback. Supplying a nonpositive vsync sequence
     * means this mode has no active Choreographer pacing. Its counters must
     * never be misinterpreted as missed vsyncs in STANDARD/RESPONSIVE modes.
     */
    fun record(glGapUs: Long, currentNativeUs: Long, vsyncSequence: Long) {
        if (glGapUs > 25_000L) {
            slowGaps++
            if (previousNativeUs > 16_667L) slowGapsAfterExpensiveNative++
            else slowGapsAfterNormalNative++
            if (currentNativeUs > 16_667L) slowGapsWithExpensiveCurrentNative++
        }
        if (vsyncSequence > 0L) {
            if (priorVsyncEnabled && lastVsyncSequence > 0L) {
                if (vsyncSequence == lastVsyncSequence) {
                    glCallbacksWithRepeatedVsync++
                } else if (vsyncSequence > lastVsyncSequence + 1L) {
                    vsyncCallbacksWithoutGl += vsyncSequence - lastVsyncSequence - 1L
                }
            }
            lastVsyncSequence = vsyncSequence
            priorVsyncEnabled = true
        } else {
            // When Choreographer is disabled, the first later vsync has no
            // previous reference; do not fabricate missed UI callbacks.
            lastVsyncSequence = 0L
            priorVsyncEnabled = false
        }
        previousNativeUs = currentNativeUs
    }

    fun snapshotAndReset(): Snapshot {
        val report = Snapshot(
            slowGaps, slowGapsAfterExpensiveNative, slowGapsAfterNormalNative,
            slowGapsWithExpensiveCurrentNative, glCallbacksWithRepeatedVsync,
            vsyncCallbacksWithoutGl,
        )
        slowGaps = 0L
        slowGapsAfterExpensiveNative = 0L
        slowGapsAfterNormalNative = 0L
        slowGapsWithExpensiveCurrentNative = 0L
        glCallbacksWithRepeatedVsync = 0L
        vsyncCallbacksWithoutGl = 0L
        // Preserve previousNativeUs and lastVsyncSequence across windows:
        // the first callback after a report is not a new playback session.
        return report
    }

    fun reset() {
        previousNativeUs = 0L
        lastVsyncSequence = 0L
        priorVsyncEnabled = false
        snapshotAndReset()
    }
}
