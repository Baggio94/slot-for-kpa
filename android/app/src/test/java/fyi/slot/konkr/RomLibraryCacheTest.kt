package fyi.slot.konkr

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RomLibraryCacheTest {
    @Test fun scanAcceptsSupportedExtensionsIndependentOfParentFolder() {
        for (ext in listOf("gba", "gb", "gbc", "zip", "GBA", "GBC", "ZIP")) {
            assertTrue(RomLibrary.isSupportedRomExtension(ext))
        }
        for (ext in listOf("ps2", "psp", "7z", "sav", "")) {
            assertFalse(RomLibrary.isSupportedRomExtension(ext))
        }
    }

    @Test fun silentRefreshComparesActualChangesWithoutDependingOnIterationOrder() {
        fun rom(name: String) = RomLibrary.IndexEntry(
            "content://rom/$name", name, "$name.gba", "GBA",
            "ABCD", false, 12345L, 1700000000L)
        val a = rom("a")
        val b = rom("b")
        org.junit.Assert.assertTrue(RomLibrary.equivalentEntries(listOf(a, b), listOf(b, a)))
        org.junit.Assert.assertFalse(RomLibrary.equivalentEntries(listOf(a, b), listOf(a)))
        org.junit.Assert.assertFalse(RomLibrary.equivalentEntries(listOf(a), listOf(a, b)))
        org.junit.Assert.assertFalse(RomLibrary.equivalentEntries(
            listOf(a), listOf(a.copy(modified=1700000001L))))
        org.junit.Assert.assertFalse(RomLibrary.equivalentEntries(listOf(a, a), listOf(a, b)))
    }

    @Test fun newlyIndexedGbHeaderRefreshesLegacyShelfOnce() {
        val legacy = RomLibrary.IndexEntry(
            "content://rom/red", "Pokémon Red", "Pokemon Red.gb", "GB",
            "", false, 524288L, 1700000000L)
        val upgraded = legacy.copy(
            gbTitle = "POKEMON RED", gbCode = "", gbJapan = false, gbCgbFlag = 0)
        assertFalse(RomLibrary.equivalentEntries(listOf(legacy), listOf(upgraded)))
        assertTrue(RomLibrary.equivalentEntries(listOf(upgraded), listOf(upgraded.copy())))
        assertFalse(RomLibrary.equivalentEntries(
            listOf(upgraded), listOf(upgraded.copy(gbCgbFlag = 0xc0))))
    }

    @Test fun unchangedFileReusesHeaderSafely() {
        assertTrue(RomLibrary.canReuseHeader("Advance Wars", 8_388_608, 1_700_000_000_000,
            "Advance Wars", 8_388_608, 1_700_000_000_000))
    }

    @Test fun changedOrUnknownMetadataAlwaysReloadsHeader() {
        assertFalse(RomLibrary.canReuseHeader("Game", 1024, 1234, "Game", 1025, 1234))
        assertFalse(RomLibrary.canReuseHeader("Game", 1024, 1234, "Game", 1024, 1235))
        assertFalse(RomLibrary.canReuseHeader("Game", 1024, 1234, "Renamed", 1024, 1234))
        assertFalse(RomLibrary.canReuseHeader("Game", 1024, 1234, "Game", 1024, 0))
        assertFalse(RomLibrary.canReuseHeader("Game", 1024, 1234, "Game", -1, 1234))
        assertFalse(RomLibrary.canReuseHeader(null, 1024, 1234, "Game", 1024, 1234))
    }
}
