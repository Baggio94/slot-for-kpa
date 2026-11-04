package fyi.slot.konkr

/**
 * Lock-free, diagnostics-only bridge between the single artwork worker and
 * Android's already-running 100ms UI pulse.
 *
 * Two sources share the shelf's one counter: the GB/GBC/GBA official art
 * pass and the NES/SNES/Mega Drive ScreenScraper pass. Each keeps its own
 * slot for the session, so "LABELS n/total" counts every cart of both and
 * no longer jumps between the two passes' own totals.
 *
 * No main-thread callbacks, no extra ROM reads, no network requests, and no
 * changes to matching, retries, attempts, writes or source/cache policies.
 */
internal object AutoCartLabelDiagnostics {
    internal data class Snapshot(
        val checked: Int = 0,
        val total: Int = 0,
        val saved: Int = 0,
        val notFound: Int = 0,
        val networkError: Int = 0,
        val invalidArtwork: Int = 0,
        val otherError: Int = 0,
    ) {
        operator fun plus(o: Snapshot) = Snapshot(checked + o.checked, total + o.total,
            saved + o.saved, notFound + o.notFound, networkError + o.networkError,
            invalidArtwork + o.invalidArtwork, otherError + o.otherError)
    }
    const val HANDHELD = 0
    const val HOME = 1
    // Replaced whole on every write: readers never see a half update.
    @Volatile private var slots = listOf(Snapshot(), Snapshot())

    fun snapshot(): Snapshot = slots.reduce { a, b -> a + b }

    fun clear() { slots = listOf(Snapshot(), Snapshot()) }

    private fun put(source: Int, value: Snapshot) {
        slots = slots.toMutableList().also { it[source.coerceIn(0, 1)] = value }
    }

    // Only the single label worker writes these snapshots.
    fun start(checked: Int, total: Int, saved: Int, source: Int = HANDHELD) {
        put(source, Snapshot(
            checked = checked.coerceAtLeast(0),
            total = total.coerceAtLeast(0),
            saved = saved.coerceAtLeast(0)))
    }

    fun result(checked: Int, saved: Int, outcome: String, source: Int = HANDHELD) {
        val old = slots[source.coerceIn(0, 1)]
        put(source, old.copy(
            checked = checked.coerceAtLeast(0),
            saved = saved.coerceAtLeast(0),
            notFound = old.notFound + if (outcome == "NOT_FOUND") 1 else 0,
            networkError = old.networkError + if (outcome == "NETWORK_ERROR") 1 else 0,
            invalidArtwork = old.invalidArtwork + if (outcome == "INVALID_ARTWORK") 1 else 0,
            otherError = old.otherError +
                if (outcome == "ERROR" || outcome == "DAT_UNAVAILABLE") 1 else 0,
        ))
    }
}
