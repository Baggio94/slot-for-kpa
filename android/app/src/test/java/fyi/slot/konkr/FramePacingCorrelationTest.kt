package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Test

class FramePacingCorrelationTest {
    @Test fun slowGlGapAfterPreviousExpensiveNativeIsCountedSeparately() {
        val probe = FramePacingCorrelation()
        probe.record(16_667, 24_000, 1L)
        probe.record(32_000, 4_000, 2L)
        probe.record(31_000, 27_000, 3L)
        val s = probe.snapshotAndReset()
        assertEquals(2L, s.slowGaps)
        assertEquals(1L, s.slowGapsAfterExpensiveNative)
        assertEquals(1L, s.slowGapsAfterNormalNative)
        assertEquals(1L, s.slowGapsWithExpensiveCurrentNative)
        assertEquals(0L, s.vsyncCallbacksWithoutGl)
    }

    @Test fun sampledVsyncSequenceCountsUiCallbacksWithoutGlWithoutTreatingAsPresentedFrames() {
        val probe = FramePacingCorrelation()
        probe.record(16_000, 1_000, 120L)
        probe.record(16_000, 1_000, 123L)
        probe.record(16_000, 1_000, 123L)
        val s = probe.snapshotAndReset()
        assertEquals(2L, s.vsyncCallbacksWithoutGl)
        assertEquals(1L, s.glCallbacksWithRepeatedVsync)
    }

    @Test fun continuousModeAndModeSwitchNeverInventMissedVsyncs() {
        val probe = FramePacingCorrelation()
        probe.record(16_000, 1_000, 900L)
        probe.record(16_000, 1_000, 0L)
        probe.record(16_000, 1_000, 1_500L)
        val s = probe.snapshotAndReset()
        assertEquals(0L, s.vsyncCallbacksWithoutGl)
        assertEquals(0L, s.glCallbacksWithRepeatedVsync)
    }

    @Test fun counterResetPreservesCrossWindowSequenceAndPreviousNative() {
        val probe = FramePacingCorrelation()
        probe.record(16_000, 20_000, 1L)
        probe.snapshotAndReset()
        probe.record(34_000, 5_000, 3L)
        val s = probe.snapshotAndReset()
        assertEquals(1L, s.slowGaps)
        assertEquals(1L, s.slowGapsAfterExpensiveNative)
        assertEquals(1L, s.vsyncCallbacksWithoutGl)
    }

    @Test fun gameEjectResetsSequenceAndPreviousCoreDuration() {
        val probe = FramePacingCorrelation()
        probe.record(16_000, 32_000, 80L)
        probe.reset()
        probe.record(30_000, 1_000, 400L)
        val s = probe.snapshotAndReset()
        assertEquals(1L, s.slowGapsAfterNormalNative)
        assertEquals(0L, s.slowGapsAfterExpensiveNative)
        assertEquals(0L, s.vsyncCallbacksWithoutGl)
    }

    @Test fun gapExactly25msAndNativeExactly16667usDoNotCountAsSlow() {
        val probe = FramePacingCorrelation()
        probe.record(25_000, 16_667, 5L)
        probe.record(25_001, 16_667, 6L)
        val s = probe.snapshotAndReset()
        assertEquals(1L, s.slowGaps)
        assertEquals(1L, s.slowGapsAfterNormalNative)
        assertEquals(0L, s.slowGapsAfterExpensiveNative)
    }
}
