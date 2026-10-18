package fyi.slot.konkr

/**
 * Lock-free, diagnostics-only bridge between the existing single-threaded
 * 1.2.0 artwork worker and Android's already-running 100ms UI pulse.
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
    )
    @Volatile private var current = Snapshot()

    fun snapshot(): Snapshot = current

    /** Reset when a new batch starts, before the scanner accesses disk. */
    fun clear() { current = Snapshot() }

    // Only AutoCartLabels.fill's single worker writes these snapshots.
    fun start(checked: Int, total: Int, saved: Int) {
        current = Snapshot(
            checked = checked.coerceAtLeast(0),
            total = total.coerceAtLeast(0),
            saved = saved.coerceAtLeast(0))
    }

    fun result(checked: Int, saved: Int, outcome: String) {
        val old = current
        current = old.copy(
            checked = checked.coerceAtLeast(0),
            saved = saved.coerceAtLeast(0),
            notFound = old.notFound + if (outcome == "NOT_FOUND") 1 else 0,
            networkError = old.networkError + if (outcome == "NETWORK_ERROR") 1 else 0,
            invalidArtwork = old.invalidArtwork + if (outcome == "INVALID_ARTWORK") 1 else 0,
            otherError = old.otherError +
                if (outcome == "ERROR" || outcome == "DAT_UNAVAILABLE") 1 else 0,
        )
    }
}
