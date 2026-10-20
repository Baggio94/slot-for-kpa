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

    @Test fun firstPrefillCanDrainOnlyAnInitialPcmSlice() {
        val samples = ShortArray(20_000)
        var firstCalls = 0
        val initial = AudioWritePolicy.writeComplete(
            samples, start = 0, length = 2048
        ) { offset, remaining ->
            firstCalls++
            assertEquals(0, offset)
            remaining
        }
        assertEquals(2048, initial)
        assertEquals(1, firstCalls)
        val rest = AudioWritePolicy.writeComplete(
            samples, start = initial, length = samples.size - initial
        ) { offset, remaining ->
            assertEquals(2048, offset)
            remaining
        }
        assertEquals(samples.size, initial + rest)
    }

    @Test fun reusedBufferShortReadNeverWritesStaleSuffix() {
        val reusable = shortArrayOf(1, 2, 3, 4, 5, 6, 99, 98, 97, 96)
        val captured = ArrayList<Short>()
        val validCount = 6 // JNI explicitly returned 6 of 10 shorts.
        val written = AudioWritePolicy.writeComplete(
            reusable, start = 0, length = validCount
        ) { offset, remaining ->
            for (i in offset until offset + remaining) captured.add(reusable[i])
            remaining
        }
        assertEquals(validCount, written)
        assertEquals(listOf<Short>(1, 2, 3, 4, 5, 6), captured)
    }

    @Test fun reusedBufferFirstPreloadAndShortTailRespectValidCount() {
        val reusable = ShortArray(4096) { index -> index.toShort() }
        val validCount = 2061 // 2048 initial shorts, then 13 after play().
        val chunks = ArrayList<Pair<Int, Int>>()
        val first = AudioWritePolicy.writeComplete(
            reusable, start = 0, length = minOf(validCount, 2048)
        ) { offset, remaining ->
            chunks.add(offset to remaining)
            remaining
        }
        val second = AudioWritePolicy.writeComplete(
            reusable, start = first, length = validCount - first
        ) { offset, remaining ->
            chunks.add(offset to remaining)
            remaining
        }
        assertEquals(validCount, first + second)
        assertEquals(listOf(0 to 2048, 2048 to 13), chunks)
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
