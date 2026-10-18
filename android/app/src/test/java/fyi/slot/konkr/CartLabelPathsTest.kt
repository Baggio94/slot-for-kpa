package fyi.slot.konkr

import org.junit.Assert.*
import org.junit.Test
import java.io.File

class CartLabelPathsTest {
    @Test fun matchesNativeUnsignedFnvForOpaqueUri() {
        val expected = File("/data/data/fyi.slot.konkr/files/Labels/dbb1dc49de8ac75d.png")
        assertEquals(expected,
            CartLabelPaths.file(File("/data/data/fyi.slot.konkr/files"),
                "content://roms/Pokemon.gba"))
    }

    @Test fun identicalFileNamesInDifferentFoldersDoNotShareArt() {
        val files = File("/tmp/app-files")
        assertNotEquals(
            CartLabelPaths.file(files, "content://storage/a/Pokemon.gba"),
            CartLabelPaths.file(files, "content://storage/b/Pokemon.gba"))
    }

    @Test fun officialArtworkMustBePlatformMatchedAndSafe() {
        assertTrue(AutoCartLabels.validArtworkPath("GBA/real/12ABCDEF.png", "GBA"))
        assertTrue(AutoCartLabels.validArtworkPath("GB/real/12345678.png", "GB"))
        assertFalse(AutoCartLabels.validArtworkPath("GBA/real/12ABCDEF.png", "GB"))
        assertFalse(AutoCartLabels.validArtworkPath("../GBA/real/12345678.png", "GBA"))
        assertFalse(AutoCartLabels.validArtworkPath("GBA/../evil.png", "GBA"))
        assertFalse(AutoCartLabels.validArtworkPath("https://example.com/evil.png", "GBA"))
        assertFalse(AutoCartLabels.validArtworkPath("GBA/real/photo.jpg", "GBA"))
    }
}
