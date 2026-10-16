package fyi.slot.konkr

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Stable SAF metadata may reuse the last uncompressed ROM MD5. */
class RaSmartCacheTest {
    private val valid = RaSmartCache.Fingerprint(
        size = 16L * 1024 * 1024,
        modified = 123456789L,
        md5 = "abcdef0123456789abcdef0123456789"
    )

    @Test fun unchangedFileReusesMd5() {
        assertTrue(RaSmartCache.canReuse(valid, valid.size, valid.modified))
    }

    @Test fun changedRomMustBeRehashed() {
        assertFalse(RaSmartCache.canReuse(valid, valid.size + 1, valid.modified))
        assertFalse(RaSmartCache.canReuse(valid, valid.size, valid.modified + 1))
        assertFalse(RaSmartCache.canReuse(valid, 0, valid.modified))
        assertFalse(RaSmartCache.canReuse(valid, valid.size, -1))
        assertFalse(RaSmartCache.canReuse(null, valid.size, valid.modified))
    }

    @Test fun cachedMd5MustBeCanonicalHex() {
        assertFalse(RaSmartCache.canReuse(valid.copy(md5 = "bad"), valid.size, valid.modified))
        assertFalse(RaSmartCache.canReuse(valid.copy(md5 = "z".repeat(32)),
            valid.size, valid.modified))
        assertFalse(RaSmartCache.canReuse(valid.copy(md5 = valid.md5.uppercase()),
            valid.size, valid.modified))
    }
}
