package fyi.slot.konkr

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

class RomSourceTest {
    private fun archive(vararg entries: Pair<String, ByteArray>): ByteArray {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { zip ->
            for ((name, bytes) in entries) {
                zip.putNextEntry(ZipEntry(name))
                zip.write(bytes)
                zip.closeEntry()
            }
        }
        return out.toByteArray()
    }

    private fun cartridge(): ByteArray =
        ByteArray(0x180) { (it and 0xFF).toByte() }

    @Test fun acceptsPlainAndZipIgnoringCase() {
        for (name in listOf("game.gba", "game.GB", "game.GbC", "GAME.ZIP"))
            assertTrue(RomSource.isAcceptedFile(name))
        for (name in listOf("game.7z", "game.rar", "game.zip.exe", "game.srm"))
            assertFalse(RomSource.isAcceptedFile(name))
    }

    @Test fun extractsGbaFromZipWithOptionalReadmeWithoutChangingContent() {
        val rom = cartridge()
        val zip = archive("readme.txt" to byteArrayOf(1,2,3),
            "ROMs/Drill Dozer.GBA" to rom)
        val head = RomSource.inspect(ByteArrayInputStream(zip), "Drill Dozer.ZIP")
        assertEquals("gba", head.extension)
        assertArrayEquals(rom.copyOf(0x150), head.bytes)
        val actual = RomSource.useRom(ByteArrayInputStream(zip), "Drill Dozer.zip") { input, ext ->
            assertEquals("gba", ext)
            input.readBytes()
        }
        assertArrayEquals(rom, actual)
    }

    @Test fun distinctEmbeddedGbcGbAndGbaExtensions() {
        val rom = cartridge()
        for (ext in listOf("gb", "gbc", "gba")) {
            val zip = archive("game.$ext" to rom)
            val result = RomSource.inspect(ByteArrayInputStream(zip), "game.zip")
            assertEquals(ext, result.extension)
            val copied = RomSource.useRom(ByteArrayInputStream(zip), "game.zip") { input, _ ->
                input.readBytes()
            }
            assertArrayEquals(rom, copied)
        }
    }

    @Test fun plainRomUsesSameBoundedIdentityBytes() {
        val rom = cartridge()
        assertEquals("gb", RomSource.inspect(ByteArrayInputStream(rom), "game.gb").extension)
        assertArrayEquals(rom, RomSource.useRom(ByteArrayInputStream(rom), "game.gb") { data, _ ->
            data.readBytes()
        })
    }

    @Test fun unsupportedOrEmptyArchiveFailsClosed() {
        for (zip in listOf(archive("readme.txt" to cartridge()), archive())) {
            try {
                RomSource.inspect(ByteArrayInputStream(zip), "bad.zip")
                throw AssertionError("Expected a ZIP rejection")
            } catch (_: IllegalArgumentException) { }
        }
    }

    @Test fun undersizedRomFailsHeaderRead() {
        val zip = archive("game.gba" to byteArrayOf(1, 2, 3))
        try {
            RomSource.inspect(ByteArrayInputStream(zip), "short.zip")
            throw AssertionError("Expected a short-ROM rejection")
        } catch (_: IllegalArgumentException) { }
    }

    @Test fun archiveInnerFileNameNeverBecomesASystemPath() {
        val rom = cartridge()
        val zip = archive("../../not-allowed/Game.GBC" to rom)
        val selection = RomSource.inspect(ByteArrayInputStream(zip), "safe-name.zip")
        assertEquals("gbc", selection.extension)
        assertEquals(0x150, selection.bytes.size)
        // No entry paths are returned for the launcher to write; it uses an
        // independently SHA-256-derived filename in Android's private cache.
    }
}
