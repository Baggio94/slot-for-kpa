package fyi.slot.konkr

import fyi.slot.konkr.ManualStateLinks.Link
import fyi.slot.konkr.ManualStateLinks.Seen
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ManualStateLinksTest {
    private fun link(stamp: String, modified: Long = 1L, size: Long = 10L) =
        Link("h$stamp", stamp, modified, size)

    @Test fun linksSurviveTheirFileFormat() {
        val links = mapOf(
            ".state" to link("2026-10-30_10-00-00"),
            ".state9" to link("2026-10-31_08-00-00", 42L, 7L),
        )
        assertEquals(links, ManualStateLinks.parse(ManualStateLinks.format(links)))
        assertEquals(emptyMap<String, Link>(), ManualStateLinks.parse(".state10\th\ts\t1\t1\nbroken"))
    }

    @Test fun aSlotRetroArchRewroteIsNoLongerUnchanged() {
        val links = mapOf(".state" to link("a"), ".state1" to link("b"))
        val seen = mapOf(
            ".state" to Seen(1L, 10L),
            ".state1" to Seen(2L, 10L),
            ".state2" to Seen(1L, 10L),
        )
        assertEquals(setOf(".state"), ManualStateLinks.unchanged(links, seen))
    }

    @Test fun aNewPolaroidTakesTheFirstEmptySlot() {
        val present = setOf(".state", ".state1", ".state3")
        assertEquals(".state2", ManualStateLinks.slotFor(emptyMap(), present) { true })
    }

    @Test fun fullSlotsGiveUpTheOldestPolaroidRetroArchLeftAlone() {
        val present = ManualStateLinks.SUFFIXES.toSet()
        val links = mapOf(
            ".state4" to link("2026-10-01_00-00-00"),
            ".state7" to link("2026-10-02_00-00-00"),
            ".state1" to link("2026-10-03_00-00-00"),
        )
        assertEquals(".state4", ManualStateLinks.slotFor(links, present) { true })
        assertEquals(".state7", ManualStateLinks.slotFor(links, present) { it != ".state4" })
        assertNull(ManualStateLinks.slotFor(links, present) { false })
        assertNull(ManualStateLinks.slotFor(emptyMap(), present) { true })
    }

    @Test fun stampsAreUtcAndNeverClash() {
        val at = 1_761_900_000_123L // 2025-10-31 08:40:00 UTC
        assertEquals("2025-10-31_08-40-00", ManualStateLinks.stampFor(at, emptySet()))
        assertEquals("2025-10-31_08-40-02", ManualStateLinks.stampFor(at,
            setOf("2025-10-31_08-40-00", "2025-10-31_08-40-01")))
    }
}
