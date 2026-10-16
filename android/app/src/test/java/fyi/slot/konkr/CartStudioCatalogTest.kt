package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertArrayEquals
import org.junit.Test
import java.io.ByteArrayInputStream

class CartStudioCatalogTest {
    @Test fun crc32MatchesOriginalSlotStudioReference() {
        val fp = CartStudioCatalog.fingerprintStream(
            ByteArrayInputStream("123456789".toByteArray()))
        assertEquals(0xCBF43926L, fp.crc)
        assertEquals("CBF43926", fp.hex)
    }

    @Test fun headersAreLimitedToTheExactOriginalStudioLength() {
        val bytes = ByteArray(100_000) { (it % 251).toByte() }
        val fp = CartStudioCatalog.fingerprintStream(ByteArrayInputStream(bytes))
        assertArrayEquals(bytes.copyOfRange(0, 0x150), fp.head)
    }

    @Test fun shortRomsAreNeverInventedOrPaddedAsValidHeaders() {
        val bytes = byteArrayOf(1, 2, 3)
        assertArrayEquals(bytes,
            CartStudioCatalog.fingerprintStream(ByteArrayInputStream(bytes)).head)
    }

    @Test fun streamingDoesNotChangeChecksumsAcrossSmallChunks() {
        val bytes = ByteArray(300_000) { (it * 7 % 251).toByte() }
        val small = object : ByteArrayInputStream(bytes) {
            override fun read(b: ByteArray, off: Int, len: Int): Int =
                super.read(b, off, minOf(len, 13))
        }
        val a = CartStudioCatalog.fingerprintStream(ByteArrayInputStream(bytes))
        val b = CartStudioCatalog.fingerprintStream(small)
        assertEquals(a.crc, b.crc)
        assertArrayEquals(a.head, b.head)
    }
}
