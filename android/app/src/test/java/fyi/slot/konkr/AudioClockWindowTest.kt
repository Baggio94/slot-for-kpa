package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AudioClockWindowTest {
    @Test fun alignedCommittedFramesAndPcmAreMeasuredFromSameWindow() {
        val start = longArrayOf(100, 100, 200000, 200000, 0, 0, 0)
        val end = longArrayOf(399, 399, 855360, 855360, 0, 0, 2)
        val window = AudioClockWindow.between(start, end, 5000L, 65536)!!
        assertEquals(299L, window.callbacks)
        assertEquals(299L, window.committedFrames)
        assertEquals(59.8, window.frameRate, 0.00001)
        assertEquals(5000.0, window.producedPcmMs, 0.001)
        assertEquals(0.0, window.producerDeficitMs, 0.001)
        assertEquals(2L, window.overdueCallbacks)
    }

    @Test fun missingCommittedFrameCannotBeHiddenByGlCallbacks() {
        val before = LongArray(AudioClockWindow.COUNTERS)
        val after = longArrayOf(300, 298, 653168, 653168, 0, 2, 1)
        val sample = AudioClockWindow.between(before, after, 5000L, 65536)!!
        assertEquals(60.0, sample.callbackRate, 0.001)
        assertEquals(59.6, sample.frameRate, 0.001)
        assertEquals(2L, sample.noStepCallbacks)
        assertTrue(sample.producerDeficitMs > 0.0)
    }

    @Test fun countersAreCumulativeAndResetsAreNotNegativeFrames() {
        assertNull(AudioClockWindow.between(longArrayOf(5), longArrayOf(8), 5000, 65536))
        val b = longArrayOf(10, 10, 100, 100, 0, 0, 0)
        val a = longArrayOf(9, 11, 200, 200, 0, 0, 0)
        assertNull(AudioClockWindow.between(b, a, 5000, 65536))
        assertNull(AudioClockWindow.between(b, b, 0, 65536))
        assertNull(AudioClockWindow.between(b, b, 5000, 0))
    }

    @Test fun emptyProducedPcmFramesAndQueuedShortsAreDistinct() {
        val b = LongArray(AudioClockWindow.COUNTERS)
        val a = longArrayOf(300, 300, 656000, 653000, 1, 0, 0)
        val x = AudioClockWindow.between(b, a, 5000, 65536)!!
        assertEquals(1L, x.emptyPcmFrames)
        assertTrue(x.producedPcmMs > x.queuedPcmMs)
        assertEquals(300L, x.committedFrames)
    }
}
