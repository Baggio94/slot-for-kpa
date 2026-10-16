package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Test

/** No ROM content or Android storage provider required: migration must be
 * deterministic and preserve two disagreeing saves on first introduction.
 */
class SharedGbaSavesTest {
    private val a = ByteArray(32768) { 0x11 }
    private val b = ByteArray(32768) { 0x22 }

    @Test fun coreSwitchDialogOnlyAppearsForRealSwitchWithDestinationAutoState() {
        val offer = RetroArchStorage::shouldOfferCoreResume
        assertEquals(true, offer(true, "mGBA", "gpSP", true))
        assertEquals(true, offer(true, "gpSP", "mGBA", true))
        assertEquals(false, offer(false, "mGBA", "gpSP", true))
        assertEquals(false, offer(true, "mGBA", "mGBA", true))
        assertEquals(false, offer(true, "gpSP", "gpSP", true))
        assertEquals(false, offer(true, "mGBA", "gpSP", false))
        assertEquals(false, offer(true, null, "gpSP", true))
        assertEquals(false, offer(true, "legacy", "gpSP", true))
    }

    @Test fun explicitCoreChoiceCanSafelyExpandErasedGbaSram() {
        val selected = ByteArray(65536) { (it and 0x7f).toByte() }
        val saved = RetroArchStorage.prepareSharedChoice(selected,
            listOf(65536, 131072))
        assertEquals(65536, saved[0].size)
        assertEquals(131072, saved[1].size)
        org.junit.Assert.assertArrayEquals(selected, saved[0])
        org.junit.Assert.assertArrayEquals(selected, saved[1].copyOf(65536))
        org.junit.Assert.assertTrue(saved[1].copyOfRange(65536, 131072)
            .all { it == 0xff.toByte() })
    }

    @Test fun explicitCoreChoiceNeverTruncatesNonErasedSaveData() {
        val selected = ByteArray(131072) { 0xff.toByte() }
        selected[90000] = 0x35
        try {
            RetroArchStorage.prepareSharedChoice(selected, listOf(65536, 131072))
            org.junit.Assert.fail("Cannot discard a real 90KiB flash sector")
        } catch (expected: IllegalStateException) {
            org.junit.Assert.assertTrue(expected.message!!.contains("without losing"))
        }
    }

    @Test fun explicitCoreChoiceRejectsMalformedSramBeforeAnyFileWrite() {
        for (size in listOf(0, 123, 2048, 262144)) {
            try {
                RetroArchStorage.prepareSharedChoice(ByteArray(size),
                    listOf(131072, 65536))
                org.junit.Assert.fail("Unsupported choice size $size")
            } catch (_: IllegalArgumentException) { }
        }
    }

    @Test fun firstMigrationNeverGuessesBetweenDifferentProgress() {
        assertEquals(RetroArchStorage.SharedGbaSource.CONFLICT,
            RetroArchStorage.chooseGbaSource(a, b, null))
    }

    @Test fun equalSramCanBeMirroredAcrossBothEmulators() {
        assertEquals(RetroArchStorage.SharedGbaSource.OWN,
            RetroArchStorage.chooseGbaSource(a, a.copyOf(), null))
    }

    @Test fun oneExistingSaveCanSeedOtherCore() {
        assertEquals(RetroArchStorage.SharedGbaSource.OTHER,
            RetroArchStorage.chooseGbaSource(null, b, null))
        assertEquals(RetroArchStorage.SharedGbaSource.OWN,
            RetroArchStorage.chooseGbaSource(a, null, null))
    }

    @Test fun previouslyPublishedSramIdentifiesWhichCoreChanged() {
        assertEquals(RetroArchStorage.SharedGbaSource.OTHER,
            RetroArchStorage.chooseGbaSource(a, b, a))
        assertEquals(RetroArchStorage.SharedGbaSource.OWN,
            RetroArchStorage.chooseGbaSource(b, a, a))
    }

    @Test fun concurrentChangesConflictRatherThanDiscardProgress() {
        val c = ByteArray(32768) { 0x33 }
        assertEquals(RetroArchStorage.SharedGbaSource.CONFLICT,
            RetroArchStorage.chooseGbaSource(b, c, a))
    }

    @Test fun firstSramMirrorCreatesAbsentOppositeCoreDirectories() {
        val dir = java.nio.file.Files.createTempDirectory("slot-shared-gba").toFile()
        try {
            val mgba = java.io.File(dir, "Saves/mGBA/Game.srm")
            val gpsp = java.io.File(dir, "Saves/gpSP/Game.srm")
            RetroArchStorage.stage(mgba, a)
            RetroArchStorage.stage(gpsp, mgba.readBytes())
            org.junit.Assert.assertArrayEquals(a, mgba.readBytes())
            org.junit.Assert.assertArrayEquals(a, gpsp.readBytes())
            org.junit.Assert.assertFalse(java.io.File(gpsp.path + ".partial").exists())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test fun reverseSramMirrorAlsoCreatesAbsentMgbaFolder() {
        val dir = java.nio.file.Files.createTempDirectory("slot-shared-gba-reverse").toFile()
        try {
            val gpsp = java.io.File(dir, "Saves/gpSP/Game.srm")
            val mgba = java.io.File(dir, "Saves/mGBA/Game.srm")
            RetroArchStorage.stage(gpsp, b)
            RetroArchStorage.stage(mgba, gpsp.readBytes())
            org.junit.Assert.assertArrayEquals(b, mgba.readBytes())
            org.junit.Assert.assertFalse(java.io.File(mgba.path + ".partial").exists())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test fun gpspAlways128KiBBaselineIsEquivalentToSmallerMgbaSaves() {
        for (nativeSize in listOf(512, 8192, 32768, 65536)) {
            val native = ByteArray(nativeSize) { (it and 0x7f).toByte() }
            val gpsp = RetroArchStorage.gbaRamAtSize(native, 131072)!!
            assertEquals(131072, gpsp.size)
            assertEquals(true, RetroArchStorage.sameGbaRam(native, gpsp))
            org.junit.Assert.assertArrayEquals(native,
                RetroArchStorage.gbaRamAtSize(gpsp, nativeSize)!!)
            assertEquals(RetroArchStorage.SharedGbaSource.OWN,
                RetroArchStorage.chooseGbaSource(native, gpsp, null))
        }
    }

    @Test fun changedProgressStillResolvesUsingPriorPublishedErasedPadding() {
        val original = ByteArray(8192) { 0x11 }
        val published = RetroArchStorage.gbaRamAtSize(original, 131072)!!
        val newGpsp = published.copyOf().also { it[2048] = 0x22 }
        assertEquals(RetroArchStorage.SharedGbaSource.OTHER,
            RetroArchStorage.chooseGbaSource(original, newGpsp, published))
        assertEquals(RetroArchStorage.SharedGbaSource.OWN,
            RetroArchStorage.chooseGbaSource(newGpsp, original, published))
        // A new gpSP save can be projected to the 8K mGBA file while
        // preserving all changed bytes. No invented timestamps involved.
        assertEquals(0x22.toByte(),
            RetroArchStorage.gbaRamAtSize(newGpsp, 8192)!![2048])
    }

    @Test fun populatedTailIsNeverDiscardedOrConsideredEquivalent() {
        val mgba = ByteArray(65536) { 0x11 }
        val gpsp = RetroArchStorage.gbaRamAtSize(mgba, 131072)!!
        gpsp[100000] = 0x45
        assertEquals(false, RetroArchStorage.sameGbaRam(mgba, gpsp))
        org.junit.Assert.assertNull(RetroArchStorage.gbaRamAtSize(gpsp, 65536))
        assertEquals(RetroArchStorage.SharedGbaSource.CONFLICT,
            RetroArchStorage.chooseGbaSource(mgba, gpsp, null))
    }

    @Test fun concurrentDifferentProgressIsStillAConflictAcrossCapacities() {
        val before = ByteArray(512) { 0x11 }
        val original = RetroArchStorage.gbaRamAtSize(before, 131072)!!
        val one = before.copyOf().also { it[1] = 0x22 }
        val other = original.copyOf().also { it[1] = 0x33 }
        assertEquals(RetroArchStorage.SharedGbaSource.CONFLICT,
            RetroArchStorage.chooseGbaSource(one, other, original))
    }

    @Test fun capacityConversionRejectsUnsupportedSizes() {
        val save = ByteArray(512)
        for (size in listOf(0, 100, 2048, 262144)) {
            org.junit.Assert.assertNull(RetroArchStorage.gbaRamAtSize(save, size))
        }
    }

    @Test fun noPreviousSaveIsSafe() {
        assertEquals(RetroArchStorage.SharedGbaSource.NONE,
            RetroArchStorage.chooseGbaSource(null, null, null))
    }
}
