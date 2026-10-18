package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Test

/** Side-channel progress must not add attempts or drive scraper decisions. */
class AutoCartLabelDiagnosticsTest {
    @Test fun separateCounterReportsOnlyRealSavedFiles() {
        AutoCartLabelDiagnostics.clear()
        AutoCartLabelDiagnostics.start(0, 5, 0)
        AutoCartLabelDiagnostics.result(1, 0, "NOT_FOUND")
        AutoCartLabelDiagnostics.result(2, 0, "NETWORK_ERROR")
        AutoCartLabelDiagnostics.result(3, 0, "INVALID_ARTWORK")
        AutoCartLabelDiagnostics.result(4, 1, "SAVED")
        val snapshot = AutoCartLabelDiagnostics.snapshot()
        assertEquals(4, snapshot.checked)
        assertEquals(5, snapshot.total)
        assertEquals(1, snapshot.saved)
        assertEquals(1, snapshot.notFound)
        assertEquals(1, snapshot.networkError)
        assertEquals(1, snapshot.invalidArtwork)
        assertEquals(0, snapshot.otherError)
    }

    @Test fun eachBoundedBatchBeginsFreshButKeepsSessionSaveCount() {
        AutoCartLabelDiagnostics.clear()
        AutoCartLabelDiagnostics.start(16, 48, 3)
        AutoCartLabelDiagnostics.result(17, 3, "DAT_UNAVAILABLE")
        AutoCartLabelDiagnostics.start(17, 48, 3)
        val snapshot = AutoCartLabelDiagnostics.snapshot()
        assertEquals(17, snapshot.checked)
        assertEquals(48, snapshot.total)
        assertEquals(3, snapshot.saved)
        assertEquals(0, snapshot.otherError)
    }
}
