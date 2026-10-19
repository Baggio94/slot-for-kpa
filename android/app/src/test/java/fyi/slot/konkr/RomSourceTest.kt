package fyi.slot.konkr

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
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

    @Test fun zipSaveStemPreservesRetroArchIdentityAndEmbeddedGbaType() {
        for (ext in listOf("gba", "gbc", "gb")) {
            val normalized = RomSource.archiveSaveFilename("Pokemon Emerald.ZIP", ext.uppercase())
            assertEquals("Pokemon Emerald.$ext", normalized)
            assertEquals(ext == "gba", normalized.endsWith(".gba", ignoreCase = true))
            assertEquals("Pokemon Emerald", normalized.substringBeforeLast('.'))
        }
        // The outer basename is authoritative, even if it already contains
        // a different extension. Existing SRAM, RTC and states use this stem.
        assertEquals("Game.gbc.gba",
            RomSource.archiveSaveFilename("Game.gbc.zip", "gba"))
        for ((name, inner) in listOf(
            "game.gba" to "gba", "game.7z" to "gba",
            "game.ZIP" to "7z", "game.ZIP" to "zip"
        )) {
            try {
                RomSource.archiveSaveFilename(name, inner)
                throw AssertionError("Expected invalid ZIP save identity rejection")
            } catch (_: IllegalArgumentException) { }
        }
    }

    @Test fun firstMatchingRomIsIdenticalForHeaderCacheAndEmulation() {
        val first = cartridge()
        val second = ByteArray(0x180) { 0x55 }
        val zipped = archive(
            "covers/front.png" to byteArrayOf(0, 1, 2),
            "docs/README.txt" to byteArrayOf(3, 4),
            "../GBA/First.GBA" to first,
            "GBA/Second.gba" to second
        )
        val head = RomSource.inspect(ByteArrayInputStream(zipped), "Outer.zip")
        assertEquals("gba", head.extension)
        assertArrayEquals(first.copyOf(RomSource.MIN_ROM_BYTES), head.bytes)
        val launchBytes = RomSource.useRom(ByteArrayInputStream(zipped), "Outer.zip") {
            input, ext ->
            assertEquals(head.extension, ext)
            input.readBytes()
        }
        assertArrayEquals(first, launchBytes)
        // A ZIP's ROM identity is the outer SAF URI, not an inner entry path.
        assertEquals("Outer.gba", RomSource.archiveSaveFilename("Outer.zip", head.extension))
    }

    @Test fun allows256thEntryAndRejectsRomBeyondEntrySafetyLimit() {
        val docs = (1..255).map { "docs/$it.txt" to byteArrayOf(1) }
        val firstArchive = archive(*(docs + ("game.gba" to cartridge())).toTypedArray())
        assertEquals("gba",
            RomSource.inspect(ByteArrayInputStream(firstArchive), "many.zip").extension)
        val tooMany = archive(*(
            docs + ("docs/256.txt" to byteArrayOf(1)) + ("game.gba" to cartridge())
        ).toTypedArray())
        try {
            RomSource.inspect(ByteArrayInputStream(tooMany), "many.zip")
            throw AssertionError("ROM beyond 256 ZIP entries must not be used")
        } catch (_: IllegalArgumentException) { }
    }

    @Test fun rejectsDeclaredOversizedZipRomWithoutAllocatingOrExtracting() {
        // ZipInputStream reads the local entry header before inflation. A
        // bogus declared 64MiB+1 ROM must be rejected before any cache write
        // or large allocation. The payload itself is intentionally absent.
        val name = "Oversized.gba".toByteArray(Charsets.UTF_8)
        val size = (RomSource.MAX_ROM_BYTES + 1).toInt()
        val header = ByteBuffer.allocate(30 + name.size).order(ByteOrder.LITTLE_ENDIAN)
        header.putInt(0x04034b50) // ZIP local file header signature
        header.putShort(20.toShort()) // version needed
        header.putShort(0.toShort()) // flags
        header.putShort(0.toShort()) // STORED
        header.putShort(0.toShort()) // time
        header.putShort(0.toShort()) // date
        header.putInt(0) // CRC
        header.putInt(size) // compressed bytes
        header.putInt(size) // uncompressed bytes
        header.putShort(name.size.toShort())
        header.putShort(0.toShort()) // extra length
        header.put(name)
        try {
            RomSource.inspect(ByteArrayInputStream(header.array()), "unsafe.zip")
            throw AssertionError("Oversized ROM was accepted")
        } catch (expected: IllegalArgumentException) {
            assertTrue(expected.message.orEmpty().contains("safe size"))
        }
    }

}
