package fyi.slot.konkr

import org.junit.Assert.*
import org.junit.Test

/** Validates the direct SAF child IDs before they reach Android's
 * ExternalStorageProvider; no filesystem access or tree escape is allowed.
 */
class RetroArchStorageDirectUriTest {
    @Test fun primaryStorageCanResolveExactRetroArchSavesAndStates() {
        assertEquals(
            "primary:RetroArch/saves/mGBA/Pokemon - Version Rubis (France) (Rev 1).srm",
            RetroArchStorage.directDocumentId("primary:RetroArch/saves",
                "mGBA", "Pokemon - Version Rubis (France) (Rev 1).srm"))
        assertEquals(
            "primary:RetroArch/states/mGBA/Advance Wars 2 - Black Hole Rising (USA).state.auto",
            RetroArchStorage.directDocumentId("primary:RetroArch/states",
                "mGBA", "Advance Wars 2 - Black Hole Rising (USA).state.auto"))
    }

    @Test fun removableStorageAndAlternativeCoreWork() {
        assertEquals("ABCD-1234:My Saves/gpSP/Game.rtc",
            RetroArchStorage.directDocumentId("ABCD-1234:My Saves", "gpSP", "Game.rtc"))
    }

    @Test fun androidMissingRtcWrappedInIllegalArgumentIsOptional() {
        val missing = "primary:RetroArch/saves/gpSP/Advance Wars (USA) (Rev 1) (No Glitch) (Bartis1989).rtc"
        val message = "Failed to determine if $missing is child of primary:RetroArch/saves: " +
            "java.io.FileNotFoundException: Missing file for $missing at /storage/emulated/0/RetroArch/saves/gpSP/Advance Wars.rtc"
        assertTrue(RetroArchStorage.isMissingDirectDocument(IllegalArgumentException(message), missing))
    }

    @Test fun missingAutoStateAndSramAreOptionalButOnlyForTheirExactDocument() {
        val state = "primary:RetroArch/states/mGBA/Advance Wars 2.state.auto"
        val sram = "primary:RetroArch/saves/gpSP/Pokemon Ruby.srm"
        val wrapped = IllegalArgumentException("Failed to determine if $state is child of primary:RetroArch/states: " +
            "java.io.FileNotFoundException: Missing file for $state at /storage/emulated/0/RetroArch/states/mGBA/Advance Wars 2.state.auto")
        assertTrue(RetroArchStorage.isMissingDirectDocument(wrapped, state))
        assertFalse(RetroArchStorage.isMissingDirectDocument(wrapped, sram))
        assertFalse(RetroArchStorage.isMissingDirectDocument(wrapped, state + ".old"))
    }

    @Test fun invalidUriAndOtherFailuresAreNeverSilenced() {
        val requested = "primary:RetroArch/saves/gpSP/Game.rtc"
        assertFalse(RetroArchStorage.isMissingDirectDocument(
            IllegalArgumentException("Invalid URI: content://com.android.externalstorage.documents/tree/primary%3ARetroArch"), requested))
        assertFalse(RetroArchStorage.isMissingDirectDocument(
            IllegalArgumentException("Failed to determine if $requested is child of root: Permission denied"), requested))
        val wrapped = IllegalArgumentException("Provider error", IllegalStateException("Unable to open URI"))
        assertFalse(RetroArchStorage.isMissingDirectDocument(wrapped, requested))
        // Only the matching child absence is handled; a different missing file
        // must not hide a genuine tree/provider misconfiguration.
        val other = "primary:RetroArch/saves/gpSP/Different.rtc"
        assertFalse(RetroArchStorage.isMissingDirectDocument(
            IllegalArgumentException("Failed to determine if $requested is child: " +
                "java.io.FileNotFoundException: Missing file for $other at /sdcard/Different.rtc"), requested))
    }

    @Test fun invalidVolumesCoreAndTraversalsMustFallBack() {
        assertNull(RetroArchStorage.directDocumentId("cloud:roms", "mGBA", "Game.srm"))
        assertNull(RetroArchStorage.directDocumentId("primary:roms", "Other", "Game.srm"))
        assertNull(RetroArchStorage.directDocumentId("primary:roms", "mGBA", "../hidden"))
        assertNull(RetroArchStorage.directDocumentId("primary:roms", "mGBA", "a/b"))
        assertNull(RetroArchStorage.directDocumentId("primary:roms", "mGBA", "."))
        assertNull(RetroArchStorage.directDocumentId("primary:roms/../outside", "mGBA", "Game.srm"))
        assertNull(RetroArchStorage.directDocumentId("primary:roms", "mGBA", "bad\\name"))
        assertNull(RetroArchStorage.directDocumentId("primary:roms", "mGBA", "bad\nname"))
    }
}
