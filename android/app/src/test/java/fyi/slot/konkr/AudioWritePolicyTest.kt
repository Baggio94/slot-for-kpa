package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Test

class AudioWritePolicyTest {
    @Test fun fullWriteUsesSingleCall() {
        var calls = 0
        val written = AudioWritePolicy.writeComplete(ShortArray(8)) { offset, remaining ->
            calls++
            assertEquals(0, offset)
            remaining
        }
        assertEquals(8, written)
        assertEquals(1, calls)
    }

    @Test fun shortWritesContinueAtExactUnwrittenOffset() {
        val offsets = ArrayList<Int>()
        val sizes = listOf(2, 4, 2)
        var index = 0
        val written = AudioWritePolicy.writeComplete(ShortArray(8)) { offset, remaining ->
            offsets.add(offset)
            val result = sizes[index++]
            assertEquals(8 - offset, remaining)
            result
        }
        assertEquals(8, written)
        assertEquals(listOf(0, 2, 6), offsets)
    }

    @Test fun zeroErrorAndInvalidCountsNeverSpin() {
        assertEquals(2, AudioWritePolicy.writeComplete(ShortArray(8)) { offset, _ ->
            if (offset == 0) 2 else 0
        })
        assertEquals(0, AudioWritePolicy.writeComplete(ShortArray(8)) { _, _ -> -3 })
        assertEquals(0, AudioWritePolicy.writeComplete(ShortArray(8)) { _, _ -> 9 })
    }

    @Test fun emptyBufferAvoidsAudioCalls() {
        var calls = 0
        assertEquals(0, AudioWritePolicy.writeComplete(shortArrayOf()) { _, _ ->
            calls++
            0
        })
        assertEquals(0, calls)
    }

    @Test fun badDriverCannotCauseUnboundedLoop() {
        var calls = 0
        assertEquals(64, AudioWritePolicy.writeComplete(ShortArray(1000)) { _, _ ->
            calls++
            1
        })
        assertEquals(64, calls)
    }
}
