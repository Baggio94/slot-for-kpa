package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AudioClockWindowTest {
    @Test fun alignedCommittedFramesAndPcmAreMeasuredFromSameWindow() {
        val start = longArrayOf(100, 100, 200000, 200000, 0, 0, 0, 10, 2000)
        val end = longArrayOf(399, 399, 855360, 855360, 0, 0, 2, 12, 2260)
        val window = AudioClockWindow.between(start, end, 5000L, 65536)!!
        assertEquals(299L, window.callbacks)
        assertEquals(299L, window.committedFrames)
        assertEquals(59.8, window.frameRate, 0.00001)
        assertEquals(5000.0, window.producedPcmMs, 0.001)
        assertEquals(0.0, window.producerDeficitMs, 0.001)
        assertEquals(2L, window.overdueCallbacks)
        assertEquals(2L, window.fifoOverflowEvents)
        assertEquals(260L, window.fifoDroppedShorts)
    }

    @Test fun missingCommittedFrameCannotBeHiddenByGlCallbacks() {
        val before = LongArray(AudioClockWindow.COUNTERS)
        val after = longArrayOf(300, 298, 653168, 653168, 0, 2, 1, 0, 0)
        val sample = AudioClockWindow.between(before, after, 5000L, 65536)!!
        assertEquals(60.0, sample.callbackRate, 0.001)
        assertEquals(59.6, sample.frameRate, 0.001)
        assertEquals(2L, sample.noStepCallbacks)
        assertTrue(sample.producerDeficitMs > 0.0)
    }

    @Test fun countersAreCumulativeAndResetsAreNotNegativeFrames() {
        assertNull(AudioClockWindow.between(longArrayOf(5), longArrayOf(8), 5000, 65536))
        val b = longArrayOf(10, 10, 100, 100, 0, 0, 0, 9, 990)
        val a = longArrayOf(9, 11, 200, 200, 0, 0, 0, 9, 990)
        assertNull(AudioClockWindow.between(b, a, 5000, 65536))
        assertNull(AudioClockWindow.between(b, b, 0, 65536))
        assertNull(AudioClockWindow.between(b, b, 5000, 0))
    }

    @Test fun emptyProducedPcmFramesAndQueuedShortsAreDistinct() {
        val b = LongArray(AudioClockWindow.COUNTERS)
        val a = longArrayOf(300, 300, 656000, 653000, 1, 0, 0, 0, 0)
        val x = AudioClockWindow.between(b, a, 5000, 65536)!!
        assertEquals(1L, x.emptyPcmFrames)
        assertTrue(x.producedPcmMs > x.queuedPcmMs)
        assertEquals(300L, x.committedFrames)
    }
    @Test fun fifoLossCountersCannotRewindAcrossAudioProbeWindows() {
        val old = longArrayOf(0, 0, 0, 0, 0, 0, 0, 4, 400)
        val next = longArrayOf(0, 0, 0, 0, 0, 0, 0, 3, 401)
        assertNull(AudioClockWindow.between(old, next, 5000L, 65536))
    }

    @Test fun fifoLossWithoutSyncLabCallbacksRemainsVisible() {
        val start = LongArray(AudioClockWindow.COUNTERS)
        val end = longArrayOf(0, 0, 0, 0, 0, 0, 0, 2, 2048)
        val window = AudioClockWindow.between(start, end, 5000L, 65536)!!
        assertEquals(0L, window.callbacks)
        assertEquals(2L, window.fifoOverflowEvents)
        assertEquals(2048L, window.fifoDroppedShorts)
    }

}
