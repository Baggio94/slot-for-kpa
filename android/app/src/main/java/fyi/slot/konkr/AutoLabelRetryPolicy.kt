package fyi.slot.konkr

/**
 * Session-local bounded retries for automatic official label acquisition.
 *
 * Successful labels and *verified* missing official artwork are terminal.
 * Transient network/DAT/ROM I/O errors get up to 4 attempts per app process,
 * with growing foreground-only delays; no permanent negative disk cache.
 *
 * This is pure Kotlin to regression-test without Android, network or ROMs.
 */
internal class AutoLabelRetryPolicy {
    companion object {
        const val MAX_ATTEMPTS = 4
        private val BACKOFF_MS = longArrayOf(12_000L, 40_000L, 120_000L)
    }

    private data class Failure(val attempts: Int, val dueAt: Long)
    private val complete = HashSet<String>()
    private val deferred = HashMap<String, Failure>()

    fun pending(key: String): Boolean = key !in complete

    fun ready(key: String, nowMs: Long): Boolean =
        pending(key) && (deferred[key]?.dueAt ?: 0L) <= nowMs

    fun finished(key: String) {
        complete.add(key)
        deferred.remove(key)
    }

    /** True when retry is scheduled; false after bounded attempts exhausted. */
    fun transient(key: String, nowMs: Long): Boolean {
        if (!pending(key)) return false
        val attempts = (deferred[key]?.attempts ?: 0) + 1
        if (attempts >= MAX_ATTEMPTS) {
            finished(key)
            return false
        }
        val delay = BACKOFF_MS[(attempts - 1).coerceIn(0, BACKOFF_MS.lastIndex)]
        deferred[key] = Failure(attempts, nowMs + delay)
        return true
    }

    fun failedAttempts(key: String): Int = deferred[key]?.attempts ?: 0

    fun waitMs(key: String, nowMs: Long): Long =
        if (!pending(key)) Long.MAX_VALUE
        else (deferred[key]?.dueAt ?: nowMs).let { (it - nowMs).coerceAtLeast(0L) }

    /** Return next candidate wait; 0 means some ROM is immediately eligible. */
    fun nextDelayMs(keys: Collection<String>, nowMs: Long): Long =
        keys.asSequence().filter(::pending).map { waitMs(it, nowMs) }.minOrNull()
            ?: Long.MAX_VALUE
}
