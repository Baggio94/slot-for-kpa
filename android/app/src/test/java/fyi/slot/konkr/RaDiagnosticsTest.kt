package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class RaDiagnosticsTest {
    @Test fun accountIdentityMatchesNativeCaseFoldedMd5() {
        assertEquals("912af0dff974604f1321254ca8ff38b6",
            RaDiagnostics.accountKey("Player"))
        assertEquals(RaDiagnostics.accountKey("Baggio94"),
            RaDiagnostics.accountKey("baggio94"))
    }

    @Test fun progressOfUnknownOrZeroBadgesIsNotFalsely100Percent() {
        assertEquals(null, RaDiagnostics.badgePercentage(0, 0))
        assertEquals(0, RaDiagnostics.badgePercentage(0, 120))
        assertEquals(8, RaDiagnostics.badgePercentage(8, 100))
        assertEquals(100, RaDiagnostics.badgePercentage(200, 100))
    }

    @Test fun eachAccountKeepsASeparateCacheAndUnlockLedger() {
        assertNotEquals(RaDiagnostics.accountKey("Baggio94"),
            RaDiagnostics.accountKey("AnotherPlayer"))
    }
}
