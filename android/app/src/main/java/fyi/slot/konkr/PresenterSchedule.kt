package fyi.slot.konkr

/** Owner-thread state machine. Only frame requests coalesce; commands remain FIFO. */
internal class PresenterSchedule {
    var resumed = false
    var surfaceReady = false
    var continuous = true
    private var requested = true
    private var pending = false
    private var closed = false

    fun request() { if (!closed) requested = true }
    fun schedule(): Boolean {
        if (closed || pending || !resumed || !surfaceReady || (!continuous && !requested)) return false
        pending = true
        return true
    }
    fun cancel() { pending = false }
    fun frame(): Boolean {
        if (!pending) return false
        pending = false
        if (closed || !resumed || !surfaceReady) return false
        requested = false
        return true
    }
    fun close() { closed = true; pending = false; requested = false }
}
