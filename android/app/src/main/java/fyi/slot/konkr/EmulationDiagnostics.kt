package fyi.slot.konkr

import android.os.SystemClock
import android.util.Log
import android.view.KeyEvent
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicLongArray

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
    // Allocation-free 1ms histogram of callback gaps. 65ms bucket saturates
    // longer pauses; quantiles are approximate and NOT display-present times.
    private val glGapMs = AtomicLongArray(66)
    private val keyToGlUsTotal = AtomicLong(0L)
    private val keyToGlCount = AtomicLong(0L)
    private val keyToGlUsMax = AtomicLong(0L)
    // Hardware press time (CLOCK_MONOTONIC ns) of the press awaiting a frame,
    // and how late Android's UI thread delivered presses to slot.
    private val pendingKeyEventNs = AtomicLong(0L)
    private val dispatchLagMsTotal = AtomicLong(0L)
    private val dispatchLagMsMax = AtomicLong(0L)
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
        if (pressed && event.repeatCount == 0) {
            pendingKeyEventNs.compareAndSet(0L, event.eventTime * 1_000_000L)
            dispatchLagMsTotal.addAndGet(dispatchLagMs)
            dispatchLagMsMax.accumulateAndGet(dispatchLagMs, java.lang.Long::max)
        }
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

    /** Returns the press time this frame is the first to read, or 0. */
    fun frame(): Long {
        val now = SystemClock.elapsedRealtimeNanos()
        if (glLastNs != 0L) {
            val gap = now - glLastNs
            if (gap > 25_000_000L) glSlow25.incrementAndGet()
            if (gap > 40_000_000L) glSlow40.incrementAndGet()
            glGapMs.incrementAndGet((gap / 1_000_000L).coerceIn(0L, 65L).toInt())
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
        return pendingKeyEventNs.getAndSet(0L)
    }

    // F9: the PCM backing array is reused. Only its [0, count) prefix
    // belongs to this callback; trailing shorts are from earlier chunks.
    fun audio(pcm: ShortArray, count: Int = pcm.size) {
        require(count in 0..pcm.size)
        if (count == 0) {
            audioEmpty.incrementAndGet()
            return
        }
        audioChunks.incrementAndGet()
        audioSamples.addAndGet(count.toLong())
        if (hasAudioTail && count >= 2) {
            // Heuristic only. Natural loud transients can also exceed this
            // threshold; correlate with underruns before interpreting.
            val differenceLeft = kotlin.math.abs(pcm[0].toInt() - lastLeftSample)
            val differenceRight = kotlin.math.abs(pcm[1].toInt() - lastRightSample)
            if (differenceLeft > 24_576 || differenceRight > 24_576)
                audioGapCandidates.incrementAndGet()
        }
        if (count >= 2) {
            lastLeftSample = pcm[count - 2].toInt()
            lastRightSample = pcm[count - 1].toInt()
            hasAudioTail = true
        }
    }

    private fun resetCounters() {
        keyCount.set(0L)
        glCount.set(0L)
        glSlow25.set(0L)
        glSlow40.set(0L)
        for (i in 0 until glGapMs.length()) glGapMs.set(i, 0L)
        keyToGlUsTotal.set(0L)
        keyToGlCount.set(0L)
        keyToGlUsMax.set(0L)
        pendingKeyEventNs.set(0L)
        dispatchLagMsTotal.set(0L)
        dispatchLagMsMax.set(0L)
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
        // Consume bucket counters only once each 5s (never sort or allocate
        // on the GL or audio threads). Millisecond buckets imply ±0.5ms error.
        val gapCounts = LongArray(glGapMs.length()) { glGapMs.getAndSet(it, 0L) }
        val gapTotal = gapCounts.sum()
        fun gapPercentileUs(percent: Int): Long {
            if (gapTotal <= 0L) return 0L
            val rank = (gapTotal * percent + 99L) / 100L
            var seen = 0L
            for (i in gapCounts.indices) {
                seen += gapCounts[i]
                if (seen >= rank) return i * 1000L + 500L
            }
            return 65_500L
        }
        Log.i(TAG, String.format(Locale.ROOT,
            "SAMPLE span_ms=%d gl_callback_fps=%.3f gl_over25=%d gl_over40=%d " +
                "gl_gap_samples=%d gl_gap_p50_us=%d gl_gap_p95_us=%d gl_gap_p99_us=%d " +
                "input_down=%d input_next_gl_count=%d input_next_gl_mean_us=%d " +
                "input_next_gl_max_us=%d input_dispatch_lag_mean_ms=%d " +
                "input_dispatch_lag_max_ms=%d audio_chunks=%d audio_pcm_shorts=%d " +
                "audio_empty=%d audio_jump_candidates=%d audio_queued_ms=%.2f " +
                "audiotrack_underruns=%d %s",
            spanMs, glRate, glSlow25.getAndSet(0L), glSlow40.getAndSet(0L),
            gapTotal, gapPercentileUs(50), gapPercentileUs(95), gapPercentileUs(99),
            keys, latencyCount, meanLatencyUs, maxLatencyUs,
            if (keys > 0L) dispatchLagMsTotal.getAndSet(0L) / keys
                else { dispatchLagMsTotal.set(0L); 0L },
            dispatchLagMsMax.getAndSet(0L),
            audioChunks.getAndSet(0L), audioSamples.getAndSet(0L),
            audioEmpty.getAndSet(0L), audioGapCandidates.getAndSet(0L),
            queuedMs, audioUnderruns, lastNative))
    }
}
