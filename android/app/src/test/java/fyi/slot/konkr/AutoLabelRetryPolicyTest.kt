package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AutoLabelRetryPolicyTest {
    @Test fun transientArtworkRetriesWithBackoffAndStopsAtFourAttempts() {
        val policy = AutoLabelRetryPolicy()
        val uri = "content://roms/game.gba"
        assertTrue(policy.ready(uri, 1000L))
        assertTrue(policy.transient(uri, 1000L))
        assertFalse(policy.ready(uri, 12_999L))
        assertTrue(policy.ready(uri, 13_000L))
        assertTrue(policy.transient(uri, 13_000L))
        assertFalse(policy.ready(uri, 52_999L))
        assertTrue(policy.ready(uri, 53_000L))
        assertTrue(policy.transient(uri, 53_000L))
        assertEquals(120_000L, policy.waitMs(uri, 53_000L))
        assertTrue(policy.ready(uri, 173_000L))
        assertFalse(policy.transient(uri, 173_000L))
        assertFalse(policy.pending(uri))
        assertFalse(policy.ready(uri, 999_999L))
    }

    @Test fun confirmedMissingAndSuccessfulLabelsAreNeverRetried() {
        val p = AutoLabelRetryPolicy()
        p.finished("officially-missing")
        p.finished("already-saved")
        assertFalse(p.pending("officially-missing"))
        assertFalse(p.ready("already-saved", 999_999L))
        assertEquals(Long.MAX_VALUE,
            p.nextDelayMs(listOf("officially-missing", "already-saved"), 1L))
    }

    @Test fun deferredArtDoesNotBlockFreshItemsAndDoesNotBurnMoreBudget() {
        val p = AutoLabelRetryPolicy()
        assertTrue(p.transient("network-timeout", 0L))
        assertEquals(0L, p.nextDelayMs(listOf("network-timeout", "new-game"), 0L))
        assertEquals(12_000L, p.nextDelayMs(listOf("network-timeout"), 0L))
        assertEquals(1, p.failedAttempts("network-timeout"))
        assertFalse(p.ready("network-timeout", 100L))
        assertEquals(1, p.failedAttempts("network-timeout"))
    }

    @Test fun successfulIndexRevalidatesWithoutCarryingOldFailures() {
        val p = AutoLabelRetryPolicy()
        assertTrue(p.transient("@official-art-index", 0L))
        assertFalse(p.ready("@official-art-index", 100L))
        p.reset("@official-art-index")
        assertTrue(p.ready("@official-art-index", 100L))
        assertEquals(0, p.failedAttempts("@official-art-index"))
    }

    @Test fun interruptionsDoNotConsumeRetries() {
        val p = AutoLabelRetryPolicy()
        // No transient() on interrupted I/O or foreground loss.
        assertTrue(p.ready("interrupted-game", 100_000L))
        assertEquals(0, p.failedAttempts("interrupted-game"))
    }
}
