package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** These assertions apply only to the test/debug APK, never production. */
class SlotBuildBehaviorDebugTest {
    @Test fun ordinaryInstallKeepsSharedStorage() {
        val policy = SlotBuildBehavior.storagePolicy(
            "fyi.slot.konkr", "content://saves", "content://states", true)
        assertEquals("content://saves", policy.savesRoot)
        assertEquals("content://states", policy.statesRoot)
        assertTrue(policy.shareGbaSaves)
        assertNull(SlotBuildBehavior.externalFolderRestrictionMessage("fyi.slot.konkr"))
    }

    @Test fun multiplayerQaInstallIsAlwaysStorageIsolated() {
        val policy = SlotBuildBehavior.storagePolicy(
            "fyi.slot.konkr.lab", "content://saves", "content://states", true)
        assertNull(policy.savesRoot)
        assertNull(policy.statesRoot)
        assertFalse(policy.shareGbaSaves)
        assertNotNull(SlotBuildBehavior.externalFolderRestrictionMessage("fyi.slot.konkr.lab"))
    }
}
