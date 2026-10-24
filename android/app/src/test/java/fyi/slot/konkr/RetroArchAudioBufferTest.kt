package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Test

class RetroArchAudioBufferTest {
    @Test fun defaultLatencyAt48kHzIsSixBlocksOf4096Bytes() {
        // RetroArch logs "[OpenSL] Setting audio latency: Block size = 4096,
        // Blocks = 6, Total = 24576 ..." on a 48 kHz device.
        assertEquals(24_576, RetroArchAudioBuffer.bytes(48_000))
        // 6144 stereo frames: 128 ms, so half full is 64 ms.
        assertEquals(128.0, RetroArchAudioBuffer.bytes(48_000) / 4 * 1000.0 / 48_000, 1e-9)
    }

    @Test fun blockCountRoundsToNearestAndNeverDropsBelowTwo() {
        assertEquals(16_384, RetroArchAudioBuffer.bytes(32_768))
        assertEquals(8_192, RetroArchAudioBuffer.bytes(8_000))
        // 64 ms: blocks of next_pow2(2048) = 2048 bytes.
        assertEquals(12_288, RetroArchAudioBuffer.bytes(48_000, latencyMs = 64))
    }
}
