package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The production behavior must never remap hardware input or mask saved folders. */
class SlotBuildBehaviorReleaseTest {
    @Test fun konKrSavesAndShareSavesRemainUnchanged() {
        val policy = SlotBuildBehavior.storagePolicy(
            "fyi.slot.konkr", "content://saves", "content://states", true)
        assertEquals("content://saves", policy.savesRoot)
        assertEquals("content://states", policy.statesRoot)
        assertTrue(policy.shareGbaSaves)
        assertNull(SlotBuildBehavior.externalFolderRestrictionMessage("fyi.slot.konkr"))
    }

    @Test fun releaseBuildContainsNoAlternateLabBehavior() {
        val policy = SlotBuildBehavior.storagePolicy(
            "fyi.slot.konkr.lab", "content://saves", "content://states", true)
        assertEquals("content://saves", policy.savesRoot)
        assertEquals("content://states", policy.statesRoot)
        assertTrue(policy.shareGbaSaves)
        assertNull(SlotBuildBehavior.externalFolderRestrictionMessage("fyi.slot.konkr.lab"))
        assertNull(SlotBuildBehavior.emulatorButton(54))
    }
}
