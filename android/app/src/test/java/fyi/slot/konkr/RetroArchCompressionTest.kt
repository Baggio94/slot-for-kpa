package fyi.slot.konkr

import org.junit.Assert.*
import org.junit.Test
import java.util.Base64

/** Independent Python zlib fixture modeled on upstream RetroArch rzip_stream.c,
 * not built by RetroArchCompression.encode(), so decoder interoperability is tested.
 */
class RetroArchCompressionTest {
    private val raw = Base64.getDecoder().decode(
        "UkFTVEFURQFNRU0gBAAAAFRFU1QAAAAARU5EIAAAAAA="
    )
    private val fixture = Base64.getDecoder().decode(
        "I1JaSVB2ASMAAAIAIAAAAAAAAAAkAAAAeJwLcgwOcQxxZfR19VVgYWBgCHENDgFSDK5+LgogGgBsJAVQ"
    )

    @Test fun independentRzipV1FixtureDecodes() {
        assertTrue(RetroArchCompression.isRzip(fixture))
        assertArrayEquals(raw, RetroArchCompression.decode(fixture))
    }

    @Test fun encoderMatchesHeaderLayoutAndRoundTripsMultipleChunks() {
        for (n in listOf(28, 65536, 131072, 131073, 300000)) {
            val state = ByteArray(n) { i -> (i * 31 + 7).toByte() }
            val rzip = RetroArchCompression.encode(state)
            assertArrayEquals(byteArrayOf(35, 82, 90, 73, 80, 118, 1, 35),
                rzip.copyOfRange(0,8))
            assertArrayEquals(state, RetroArchCompression.decode(rzip))
            // Declared RetroArch chunk size is 128 KiB.
            assertEquals(0, rzip[8].toInt() and 255)
            assertEquals(0, rzip[9].toInt() and 255)
            assertEquals(2, rzip[10].toInt() and 255)
            assertEquals(0, rzip[11].toInt() and 255)
        }
    }

    @Test fun existingRastateMustBeCompleteBeforeOverwrite() {
        RetroArchCompression.requireSupportedContainer(raw)
        RetroArchCompression.requireSupportedContainer(fixture)
        RetroArchCompression.requireSupportedContainer(RetroArchCompression.encode(raw))
        assertThrows(IllegalArgumentException::class.java) {
            RetroArchCompression.requireSupportedContainer("unknown legacy state".toByteArray())
        }
        assertThrows(IllegalArgumentException::class.java) {
            RetroArchCompression.requireSupportedContainer(byteArrayOf(82,65,83,84,65,84,69,1))
        }
        val v2 = fixture.copyOf()
        v2[6] = 2
        assertThrows(IllegalArgumentException::class.java) {
            RetroArchCompression.requireSupportedContainer(v2)
        }
        val truncated = raw.copyOfRange(0, raw.size - 8)
        assertThrows(IllegalStateException::class.java) {
            RetroArchCompression.requireSupportedContainer(truncated)
        }
    }

    @Test fun optionalAchievementBlockIsCompatible() {
        val state = raw.copyOfRange(0, raw.size - 8) +
            byteArrayOf(65,67,72,86,76,0,0,0) +
            ByteArray(76) + ByteArray(4) +
            raw.copyOfRange(raw.size - 8, raw.size)
        RetroArchCompression.requireSupportedContainer(state)
        val zipped = RetroArchCompression.encode(state)
        assertArrayEquals(state, RetroArchCompression.decode(zipped))
        RetroArchCompression.requireSupportedContainer(zipped)
        assertThrows(IllegalArgumentException::class.java) {
            RetroArchCompression.requireSupportedContainer(state + byteArrayOf(1))
        }
    }

    @Test fun corruptTruncatedAndFutureVersionAreNeverAccepted() {
        assertThrows(IllegalArgumentException::class.java) {
            RetroArchCompression.decode(fixture.copyOfRange(0, 22))
        }
        val v2 = fixture.copyOf()
        v2[6] = 2
        assertThrows(IllegalArgumentException::class.java) {
            RetroArchCompression.decode(v2)
        }
        val badSize = fixture.copyOf()
        badSize[8] = 0
        badSize[9] = 0
        badSize[10] = 0
        badSize[11] = 0
        assertThrows(IllegalArgumentException::class.java) {
            RetroArchCompression.decode(badSize)
        }
    }
}
