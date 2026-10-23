package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AudioClockWindowTest {
    @Test fun fifoLossIsMeasuredBetweenProbes() {
        val window = AudioClockWindow.between(longArrayOf(10, 2000), longArrayOf(12, 2260))!!
        assertEquals(2L, window.fifoOverflowEvents)
        assertEquals(260L, window.fifoDroppedShorts)
    }

    @Test fun countersAreCumulativeAndCannotRewind() {
        assertNull(AudioClockWindow.between(longArrayOf(5), longArrayOf(8)))
        assertNull(AudioClockWindow.between(longArrayOf(4, 400), longArrayOf(3, 401)))
        assertNull(AudioClockWindow.between(longArrayOf(4, 400), longArrayOf(4, 399)))
    }
}
