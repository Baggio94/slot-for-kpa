package fyi.slot.konkr

import android.os.SystemClock
import android.util.Log
import android.view.KeyEvent
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong

/**
 * Read-only emulator instrumentation. Called from UI/GL/audio threads.
 * No CPU/GPU readback, I/O or allocations on the per-frame/per-audio path.
 * Structured reports are emitted only once every 5 seconds on the UI thread.
 *
 * The input-to-next-GL-callback measurement is NOT photon latency: it omits
 * the core's reaction, EGL swap, display scanout and controller electronics.
 */
internal object EmulationDiagnostics {
    private const val TAG = "SlotEmuDiag"
    private val pendingKeyDownNs = AtomicLong(0L)
    private val keyCount = AtomicLong(0L)
    private val glCount = AtomicLong(0L)
    private val glSlow25 = AtomicLong(0L)
    private val glSlow40 = AtomicLong(0L)
    private val keyToGlUsTotal = AtomicLong(0L)
    private val keyToGlCount = AtomicLong(0L)
    private val keyToGlUsMax = AtomicLong(0L)
    private val audioChunks = AtomicLong(0L)
    private val audioSamples = AtomicLong(0L)
    private val audioEmpty = AtomicLong(0L)
    private val audioGapCandidates = AtomicLong(0L)
    @Volatile private var glLastNs = 0L
    @Volatile private var lastRightSample = 0
    @Volatile private var lastLeftSample = 0
    @Volatile private var hasAudioTail = false
    private var lastReportAt = 0L
    private var lastNative = ""

    fun input(code: Int, pressed: Boolean, event: KeyEvent, mapped: Int) {
        if (pressed && event.repeatCount == 0) {
            keyCount.incrementAndGet()
            pendingKeyDownNs.set(SystemClock.elapsedRealtimeNanos())
        }
        val dispatchLagMs = (SystemClock.uptimeMillis() - event.eventTime)
            .coerceAtLeast(0L)
        // Per-event Log.i in production adds unpredictable work to the
        // latency-critical input callback; only the AVD debug build emits
        // detailed key logs. Release beta still aggregates counters at 5s.
        if (SlotBuildBehavior.verboseEmulationInput() && event.repeatCount == 0) {
            Log.i(TAG, "INPUT event=${if (pressed) "down" else "up"} " +
                "key=${KeyEvent.keyCodeToString(code)} mapped=${KeyEvent.keyCodeToString(mapped)} " +
                "device=${event.deviceId} scan=${event.scanCode} " +
                "dispatch_lag_ms=$dispatchLagMs")
        }
    }

    fun frame() {
        val now = SystemClock.elapsedRealtimeNanos()
        if (glLastNs != 0L) {
            val gap = now - glLastNs
            if (gap > 25_000_000L) glSlow25.incrementAndGet()
            if (gap > 40_000_000L) glSlow40.incrementAndGet()
        }
        glLastNs = now
        glCount.incrementAndGet()
        val inputNs = pendingKeyDownNs.getAndSet(0L)
        if (inputNs > 0 && now >= inputNs) {
            val delayUs = (now - inputNs) / 1000L
            keyToGlUsTotal.addAndGet(delayUs)
            keyToGlCount.incrementAndGet()
            keyToGlUsMax.accumulateAndGet(delayUs, java.lang.Long::max)
        }
    }

    fun audio(pcm: ShortArray) {
        if (pcm.isEmpty()) {
            audioEmpty.incrementAndGet()
            return
        }
        audioChunks.incrementAndGet()
        audioSamples.addAndGet(pcm.size.toLong())
        if (hasAudioTail && pcm.size >= 2) {
            // Heuristic only. Natural loud transients can also exceed this
            // threshold; correlate with underruns before interpreting.
            val differenceLeft = kotlin.math.abs(pcm[0].toInt() - lastLeftSample)
            val differenceRight = kotlin.math.abs(pcm[1].toInt() - lastRightSample)
            if (differenceLeft > 24_576 || differenceRight > 24_576)
                audioGapCandidates.incrementAndGet()
        }
        if (pcm.size >= 2) {
            lastLeftSample = pcm[pcm.size - 2].toInt()
            lastRightSample = pcm[pcm.size - 1].toInt()
            hasAudioTail = true
        }
    }

    private fun resetCounters() {
        keyCount.set(0L)
        glCount.set(0L)
        glSlow25.set(0L)
        glSlow40.set(0L)
        keyToGlUsTotal.set(0L)
        keyToGlCount.set(0L)
        keyToGlUsMax.set(0L)
        audioChunks.set(0L)
        audioSamples.set(0L)
        audioEmpty.set(0L)
        audioGapCandidates.set(0L)
    }

    @Synchronized
    fun report(isPlaying: Boolean, native: String?, queuedShorts: Int,
               sampleRate: Int, audioUnderruns: Int) {
        val now = SystemClock.elapsedRealtime()
        if (!isPlaying) {
            lastReportAt = 0L
            lastNative = ""
            glLastNs = 0L
            hasAudioTail = false
            resetCounters()
            return
        }
        if (!native.isNullOrBlank()) lastNative = native
        if (lastReportAt == 0L) {
            // Start a clean measurement window; menu navigation and the
            // initial audio queue cannot pollute later comparison trials.
            resetCounters()
            lastReportAt = now
            return
        }
        if (now - lastReportAt < 5_000L) return
        val spanMs = (now - lastReportAt).coerceAtLeast(1L)
        lastReportAt = now
        val frames = glCount.getAndSet(0L)
        val keys = keyCount.getAndSet(0L)
        val latencyCount = keyToGlCount.getAndSet(0L)
        val meanLatencyUs = if (latencyCount > 0L)
            keyToGlUsTotal.getAndSet(0L) / latencyCount
            else { keyToGlUsTotal.set(0L); 0L }
        val maxLatencyUs = keyToGlUsMax.getAndSet(0L)
        val queuedMs = if (sampleRate > 0)
            queuedShorts.toDouble() * 500.0 / sampleRate else 0.0
        val glRate = frames * 1000.0 / spanMs
        Log.i(TAG, String.format(Locale.ROOT,
            "SAMPLE span_ms=%d gl_callback_fps=%.3f gl_over25=%d gl_over40=%d " +
                "input_down=%d input_next_gl_count=%d input_next_gl_mean_us=%d " +
                "input_next_gl_max_us=%d audio_chunks=%d audio_pcm_shorts=%d " +
                "audio_empty=%d audio_jump_candidates=%d audio_queued_ms=%.2f " +
                "audiotrack_underruns=%d %s",
            spanMs, glRate, glSlow25.getAndSet(0L), glSlow40.getAndSet(0L),
            keys, latencyCount, meanLatencyUs, maxLatencyUs,
            audioChunks.getAndSet(0L), audioSamples.getAndSet(0L),
            audioEmpty.getAndSet(0L), audioGapCandidates.getAndSet(0L),
            queuedMs, audioUnderruns, lastNative))
    }
}
