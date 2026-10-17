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
}
