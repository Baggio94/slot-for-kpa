package fyi.slot.konkr

import org.junit.Assert.*
import org.junit.Test

class SlotUpdatePolicyTest {
    private val day = SlotUpdatePolicy.DAY_MS
    @Test fun automaticChecksRunOnlyAfter24Hours() {
        assertFalse(SlotUpdatePolicy.due(day, 1, 0))
        assertTrue(SlotUpdatePolicy.due(day + 1, 1, 0))
        assertFalse(SlotUpdatePolicy.due(day + 1, 1, day))
        assertTrue(SlotUpdatePolicy.due(day + SlotUpdatePolicy.RETRY_MS + 1, 1, day))
        assertTrue(SlotUpdatePolicy.due(day, 0, 0))
    }

    @Test fun onlyPublishedStableIsEligibleForStableInstallations() {
        assertTrue(SlotUpdatePolicy.eligible("1.1.0", "1.2.0", false))
        assertFalse(SlotUpdatePolicy.eligible("1.1.0", "1.2.0-beta2", true))
        assertFalse(SlotUpdatePolicy.eligible("1.1.0", "1.2.0-beta2", false))
        assertFalse(SlotUpdatePolicy.eligible("1.2.0", "1.2.0-beta3", true))
    }

    @Test fun betaCanUpdateToAnotherBetaOrNewStable() {
        assertTrue(SlotUpdatePolicy.eligible("1.2.0-beta2", "1.2.0-beta3", true))
        assertTrue(SlotUpdatePolicy.eligible("1.2.0-beta2", "1.2.0", false))
        assertFalse(SlotUpdatePolicy.eligible("1.2.0-beta2", "1.1.0", false))
        assertFalse(SlotUpdatePolicy.newer("1.2.0-beta1", "1.2.0-beta2"))
        assertTrue(SlotUpdatePolicy.newer("1.2.0-beta10", "1.2.0-beta2"))
        assertFalse(SlotUpdatePolicy.newer("1.2.0-beta2", "1.2.0-beta10"))
    }

    @Test fun invalidOrForgedVersionCannotBeAnUpgrade() {
        assertFalse(SlotUpdatePolicy.newer("beta-latest", "1.1.0"))
        assertFalse(SlotUpdatePolicy.newer("1.2.0-extra", "1.2.0"))
        assertFalse(SlotUpdatePolicy.newer("1.1.0", "1.1.0"))
        assertFalse(SlotUpdatePolicy.newer("konkr-v1.1.0", "1.2.0-beta2"))
    }

    @Test fun installedStableGreaterThanOlderRelease() {
        assertTrue(SlotUpdatePolicy.newer("konkr-v1.2.0", "1.1.0"))
        assertTrue(SlotUpdatePolicy.newer("1.3.0", "1.2.99"))
    }

    @Test fun bothRepositoryNamesAreAccepted() {
        assertTrue(SlotUpdatePolicy.releasePage("https://github.com/Baggio94/slot-for-kpa/releases/tag/konkr-v1.3.5"))
        assertTrue(SlotUpdatePolicy.releasePage("https://github.com/Baggio94/slot-for-android/releases/tag/v1.3.6"))
        assertFalse(SlotUpdatePolicy.releasePage("https://github.com/Baggio94/slot-for-androidx/releases/tag/v1.3.6"))
        assertFalse(SlotUpdatePolicy.releasePage("https://github.com/someone/slot-for-android/releases/tag/v1.3.6"))
        assertTrue(SlotUpdatePolicy.assetPath("/Baggio94/slot-for-kpa/releases/download/konkr-v1.3.5/a.apk"))
        assertTrue(SlotUpdatePolicy.assetPath("/Baggio94/slot-for-android/releases/download/v1.3.6/a.apk"))
        assertFalse(SlotUpdatePolicy.assetPath("/Baggio94/slot-for-android/archive/v1.3.6.zip"))
    }

    @Test fun oldAndNewTagsNameTheirVersion() {
        assertEquals("1.3.5", SlotUpdatePolicy.tagVersion("konkr-v1.3.5"))
        assertEquals("1.3.6", SlotUpdatePolicy.tagVersion("v1.3.6"))
        assertEquals("1.3.6-beta2", SlotUpdatePolicy.tagVersion("v1.3.6-beta2"))
        assertNull(SlotUpdatePolicy.tagVersion("1.3.6"))
        assertNull(SlotUpdatePolicy.tagVersion("konkr-1.3.6"))
        assertNull(SlotUpdatePolicy.tagVersion("other-v1.3.6"))
    }

    @Test fun newApkNameIsPreferredOverTheOldOne() {
        assertEquals(2, SlotUpdatePolicy.apkRank("slot-for-android-1.3.6-arm64.apk", "1.3.6"))
        assertEquals(1, SlotUpdatePolicy.apkRank("slot-for-konkr-1.3.6-arm64.apk", "1.3.6"))
        assertEquals(0, SlotUpdatePolicy.apkRank("slot-for-android-1.3.5-arm64.apk", "1.3.6"))
        assertEquals(0, SlotUpdatePolicy.apkRank("SHA256SUMS.txt", "1.3.6"))
    }
}
