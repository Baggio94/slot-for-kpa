package fyi.slot.konkr

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.util.AtomicFile
import android.util.Log
import org.json.JSONObject
import java.io.ByteArrayOutputStream

/**
 * Conservative automatic cartridge label acquisition. Exact ROM CRC first,
 * then conservative No-Intro title + platform + metadata matches from
 * the SAME DAT used in original Cart Studio. No fuzzy artwork guesses.
 * Manual and Studio labels always win.
 *
 * Low priority and bounded: at most eight new labels per batch, 16 attempts,
 * no work during gameplay or Android background sleep, and no ROM/BIOS writes.
 */
internal object AutoCartLabels {
    private const val TAG = "SlotAutoLabels"
    private const val MAX_INDEX = 12 * 1024 * 1024
    private const val MAX_SOURCE = 6 * 1024 * 1024
    private const val MAX_LABEL = 2 * 1024 * 1024
    private const val MAX_ATTEMPTS = 16
    private const val MAX_SAVED = 8
    private val safeArtPath = Regex("[a-zA-Z0-9_./-]+")
    private val pngSignature = byteArrayOf(-119, 80, 78, 71, 13, 10, 26, 10)

    // Only the serial artwork worker mutates this session state. Official
    // negatives never retry; transient failures retry at most four times.
    private val retry = AutoLabelRetryPolicy()
    private const val INDEX_KEY = "@official-art-index"
    private var savedTotal = 0
    @Volatile private var batchDelayMs = 4_000L

    /** Read only from MainActivity after fill() returns on its single worker. */
    fun nextBatchDelayMs(): Long = batchDelayMs

    private fun reschedule(pending: List<CartStudioCatalog.Rom>,
                           shouldContinue: () -> Boolean): Boolean {
        if (!shouldContinue() || !retry.pending(INDEX_KEY)) return false
        val remaining = pending.filter { retry.pending(it.uri) }
        if (remaining.isEmpty()) return false
        val now = android.os.SystemClock.elapsedRealtime()
        batchDelayMs = maxOf(4_000L,
            retry.waitMs(INDEX_KEY, now),
            retry.nextDelayMs(remaining.map { it.uri }, now))
            .coerceAtMost(180_000L)
        Log.i(TAG, "Pending automatic labels: ${remaining.size}; next pass in ${batchDelayMs}ms")
        return true
    }

    private fun indexFailure(pending: List<CartStudioCatalog.Rom>,
                             shouldContinue: () -> Boolean, reason: String): Boolean {
        if (!shouldContinue()) return false // Pauses aren't failed attempts.
        val allowed = retry.transient(INDEX_KEY, android.os.SystemClock.elapsedRealtime())
        Log.w(TAG, "Official art index temporarily unavailable: $reason; " +
            if (allowed) "retry scheduled" else "retry budget exhausted until app restart")
        return allowed && reschedule(pending, shouldContinue)
    }

    /** Returns true if more bounded foreground work is pending, including retries. */
    fun fill(context: Context, shouldContinue: () -> Boolean, onSaved: (String) -> Unit): Boolean {
        if (!shouldContinue()) return false
        batchDelayMs = 4_000L
        val games = CartStudioCatalog.games(context)
        if (games.isEmpty()) {
            Log.i(TAG, "Deferred: cached ROM library not ready (zero indexed games)")
            return false
        }
        val missing = games.filter { !CartLabelPaths.file(context.filesDir, it.uri).exists() }
        val pending = missing.filter { retry.pending(it.uri) }
        if (pending.isEmpty()) {
            Log.i(TAG, "No pending official art: games=${games.size}, missing=${missing.size}, savedTotal=$savedTotal")
            return false
        }
        var checked = games.count { !retry.pending(it.uri) }
        AutoCartLabelDiagnostics.start(checked, checked + pending.size, savedTotal)

        val now = android.os.SystemClock.elapsedRealtime()
        if (!retry.ready(INDEX_KEY, now)) return reschedule(pending, shouldContinue)
        val due = pending.filter { retry.ready(it.uri, now) }
        if (due.isEmpty()) return reschedule(pending, shouldContinue)

        // The same official cache, index, matching and PNG conversion as 1.2.1.
        // The only new behavior is how failed attempts are classified/scheduled.
        val indexBytes = try {
            val response = CartStudioArtProxy(context).intercept("index.json")
            if (response.statusCode != 200)
                return indexFailure(pending, shouldContinue, "HTTP ${response.statusCode}")
            response.data.use { input ->
                val output = ByteArrayOutputStream()
                val buffer = ByteArray(16 * 1024)
                while (true) {
                    if (!shouldContinue()) return false
                    val n = input.read(buffer)
                    if (n < 0) break
                    if (output.size() + n > MAX_INDEX) {
                        return indexFailure(pending, shouldContinue, "index exceeds size limit")
                    }
                    output.write(buffer, 0, n)
                }
                output.toByteArray()
            }
        } catch (error: Exception) {
            Log.w(TAG, "Official art index request failed", error)
            return indexFailure(pending, shouldContinue, error.javaClass.simpleName)
        }
        val index = try { JSONObject(String(indexBytes, Charsets.UTF_8)) }
            catch (error: Exception) {
                Log.w(TAG, "Official art index could not be parsed", error)
                return indexFailure(pending, shouldContinue, "invalid JSON")
            }
        retry.reset(INDEX_KEY)
        // Index success clears its temporary outage budget; future batches
        // still use the same official index/cache and can check connectivity.
        Log.i(TAG, "Official art index ready: CRC entries=${index.length()}")

        val art = CartStudioArtProxy(context)
        val dats = HashMap<String, OfficialArtMatcher.Catalog?>()
        var attempted = 0
        var saved = 0
        var fetched = 0
        var notFound = 0
        var errors = 0
        for (rom in due) {
            if (!shouldContinue() || attempted >= MAX_ATTEMPTS || saved >= MAX_SAVED) break
            val target = CartLabelPaths.file(context.filesDir, rom.uri)
            if (target.exists()) {
                retry.finished(rom.uri)
                continue
            }
            attempted++
            var outcome = "NOT_FOUND"
            try {
                val crc = CartStudioCatalog.cached(context, rom)
                    ?: CartStudioCatalog.identify(context, rom)
                if (!shouldContinue()) {
                    outcome = "INTERRUPTED"
                    break
                }
                val direct = OfficialArtResolver.artPath(index, rom.platform, crc.hex)
                // Kotlin getOrPut() re-invokes the initializer when it returns
                // null. Preserve null failures within THIS pass explicitly,
                // to avoid repeated 9s+12s DAT timeouts on each of 16 carts.
                if (direct == null && !dats.containsKey(rom.platform)) {
                    dats[rom.platform] = OfficialDatSource.load(
                        context, rom.platform, shouldContinue)
                }
                if (!shouldContinue()) {
                    outcome = "INTERRUPTED"
                    break
                }
                val dat = if (direct != null) null else dats[rom.platform]
                if (direct == null && dat == null) {
                    outcome = "DAT_UNAVAILABLE"
                    continue
                }
                val resolved = OfficialArtResolver.match(rom, crc, index, dat)
                val path = resolved?.let {
                    OfficialArtResolver.artPath(index, rom.platform, it)
                } ?: continue // Verified official no-match: session-terminal.
                val response = art.intercept(path)
                if (response.statusCode != 200) {
                    outcome = when (response.statusCode) {
                        404 -> "NOT_FOUND"
                        else -> "NETWORK_ERROR"
                    }
                    errors++
                    continue
                }
                outcome = "NETWORK_ERROR" // Partial reads are retryable.
                val bytes = response.data.use { input ->
                    val out = ByteArrayOutputStream()
                    val buffer = ByteArray(16 * 1024)
                    while (true) {
                        if (!shouldContinue()) break
                        val n = input.read(buffer)
                        if (n < 0) break
                        if (out.size() + n > MAX_SOURCE) break
                        out.write(buffer, 0, n)
                    }
                    out.toByteArray()
                }
                if (!shouldContinue()) {
                    outcome = "INTERRUPTED"
                    break
                }
                fetched++
                if (bytes.size !in 8..MAX_SOURCE ||
                    !bytes.copyOfRange(0, 8).contentEquals(pngSignature)) {
                    outcome = "INVALID_ARTWORK"
                    continue
                }
                val label = cropPhysicalCartLabel(bytes, rom.platform)
                if (label == null) {
                    outcome = "INVALID_ARTWORK"
                    continue
                }
                if (label.size > MAX_LABEL) {
                    outcome = "INVALID_ARTWORK"
                    continue
                }
                if (target.exists()) {
                    outcome = "EXISTS"
                    continue
                }
                if (!shouldContinue()) {
                    outcome = "INTERRUPTED"
                    break
                }
                target.parentFile?.mkdirs()
                if (target.exists()) {
                    outcome = "EXISTS"
                    continue
                }
                val atomic = AtomicFile(target)
                val stream = atomic.startWrite()
                try {
                    stream.write(label)
                    atomic.finishWrite(stream)
                } catch (error: Exception) {
                    atomic.failWrite(stream)
                    throw error
                }
                saved++
                savedTotal++
                outcome = "SAVED"
                onSaved(rom.uri)
                Thread.sleep(120L)
            } catch (error: Exception) {
                if (outcome != "SAVED") outcome = "ERROR"
                errors++
                Log.w(TAG, "Auto-art deferred for ${rom.title.take(80)}", error)
            } finally {
                val wasPending = retry.pending(rom.uri)
                // A real stored label always wins, including when the user
                // navigated away just after a successful atomic write.
                if (target.exists()) retry.finished(rom.uri)
                else if (shouldContinue()) {
                    when (outcome) {
                        "NOT_FOUND", "INVALID_ARTWORK", "EXISTS" ->
                            retry.finished(rom.uri)
                        "DAT_UNAVAILABLE", "NETWORK_ERROR", "ERROR" -> {
                            val scheduled = retry.transient(
                                rom.uri, android.os.SystemClock.elapsedRealtime())
                            Log.i(TAG, "ART_RETRY ${rom.title.take(72)}: " +
                                if (scheduled) "scheduled (attempt ${retry.failedAttempts(rom.uri)}/" +
                                    "${AutoLabelRetryPolicy.MAX_ATTEMPTS})"
                                else "exhausted for this app process")
                        }
                        // Interrupted work never burns a retry allowance.
                    }
                }
                if (wasPending && !retry.pending(rom.uri)) checked++
                AutoCartLabelDiagnostics.result(checked, savedTotal, outcome)
                if (outcome == "NOT_FOUND") notFound++
                Log.i(TAG, "ART_RESULT $outcome [${rom.platform}] ${rom.title.take(96)}")
            }
        }
        try { CartStudioCatalog.flush(context) }
        catch (error: Exception) { Log.w(TAG, "Could not persist cartridge checksums", error) }

        Log.i(TAG, "Art pass: attempted=$attempted, saved=$saved, fetched=$fetched, " +
            "no_official_match=$notFound, retryable_errors=$errors")
        return reschedule(pending, shouldContinue)
    }

    internal fun validArtworkPath(path: String, platform: String): Boolean =
        path.isNotBlank() && path.length <= 150 &&
            safeArtPath.matches(path) && !path.startsWith("/") &&
            !path.split('/').contains("..") && path.endsWith(".png") &&
            path.startsWith("$platform/")

    /**
     * Mirror the original Cart Studio Label.full: center-cover the *complete*
     * official support-texture onto the platform's label well, and composite
     * transparency over black. The source is already the label media: the
     * earlier hard-coded "physical cartridge scan" aspect checks and 137..865
     * percent crop rejected valid images and cut off real label artwork.
     *
     * Retain the low-resolution Android output sizes and the bounded decode
     * budget: no custom/Studio label or ROM bytes are changed.
     */
    private fun cropPhysicalCartLabel(bytes: ByteArray, platform: String): ByteArray? {
        val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)
        val w = opts.outWidth
        val h = opts.outHeight
        if (w !in 64..4096 || h !in 64..4096 ||
            w.toLong() * h.toLong() > 12_000_000L) {
            Log.i(TAG, "Unsupported official source dimensions: ${w}x${h} for $platform")
            return null
        }
        val source = BitmapFactory.decodeByteArray(bytes, 0, bytes.size) ?: run {
            Log.i(TAG, "Official source PNG could not be decoded: ${w}x${h}")
            return null
        }
        return try {
            val (tw, th) = when (platform) {
                "GBA" -> 264 to 136
                "GB" -> 220 to 195
                "GBC" -> 223 to 199
                else -> return null
            }
            val coverScale = maxOf(tw.toFloat() / w, th.toFloat() / h)
            val dw = w * coverScale
            val dh = h * coverScale
            val left = (tw - dw) / 2f
            val top = (th - dh) / 2f
            val result = Bitmap.createBitmap(tw, th, Bitmap.Config.ARGB_8888)
            try {
                val canvas = Canvas(result)
                canvas.drawColor(Color.BLACK)
                canvas.drawBitmap(source, null, RectF(left, top, left + dw, top + dh),
                    Paint(Paint.FILTER_BITMAP_FLAG))
                val out = ByteArrayOutputStream()
                if (!result.compress(Bitmap.CompressFormat.PNG, 100, out)) null
                else out.toByteArray()
            } finally {
                result.recycle()
            }
        } finally {
            source.recycle()
        }
    }
}
