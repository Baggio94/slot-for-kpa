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

    // Exactly one serial worker owns these process-local budgets. A 16-ROM
    // batch must not repeatedly get stuck on the same unmatched first games.
    // Seen identities are not persisted across app restarts or library resets.
    private val examinedThisSession = HashSet<String>()
    private var savedTotal = 0

    /** Returns true if another small, foreground-only batch is worthwhile. */
    fun fill(context: Context, shouldContinue: () -> Boolean, onSaved: (String) -> Unit): Boolean {
        if (!shouldContinue()) return false
        val games = CartStudioCatalog.games(context)
        if (games.isEmpty()) {
            Log.i(TAG, "Deferred: cached ROM library not ready (zero indexed games)")
            return false
        }
        val missing = games.filter { !CartLabelPaths.file(context.filesDir, it.uri).exists() }
        val pending = missing.filter { it.uri !in examinedThisSession }
        if (pending.isEmpty()) {
            Log.i(TAG, "No pending artwork: games=${games.size}, missing=${missing.size}, " +
                "examined=${examinedThisSession.size}, savedTotal=$savedTotal")
            return false
        }
        Log.i(TAG, "Starting automatic labels: games=${games.size}, missing=${missing.size}, " +
            "pending=${pending.size}, savedTotal=$savedTotal")
        // Telemetry only: no network, disk access or UI callback.
        AutoCartLabelDiagnostics.start(
            examinedThisSession.size,
            examinedThisSession.size + pending.size,
            savedTotal)

        // Original Studio's HTTPS allowlist, disk cache and bounded response.
        // An unavailable index remains retryable when connectivity returns.
        val indexResponse = CartStudioArtProxy(context).intercept("index.json")
        if (indexResponse.statusCode != 200) {
            Log.w(TAG, "Official art index unavailable: HTTP ${indexResponse.statusCode}")
            return false
        }
        val indexBytes = indexResponse.data.use { input ->
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(16 * 1024)
            while (true) {
                if (!shouldContinue()) return false
                val n = input.read(buffer)
                if (n < 0) break
                if (output.size() + n > MAX_INDEX) {
                    Log.w(TAG, "Official art index exceeds the ${MAX_INDEX} byte limit")
                    return false
                }
                output.write(buffer, 0, n)
            }
            output.toByteArray()
        }
        val index = try { JSONObject(String(indexBytes, Charsets.UTF_8)) }
            catch (error: Exception) {
                Log.w(TAG, "Official art index could not be parsed", error)
                return false
            }
        Log.i(TAG, "Official art index ready: bytes=${indexBytes.size}, CRC entries=${index.length()}")

        val art = CartStudioArtProxy(context)
        val dats = HashMap<String, OfficialArtMatcher.Catalog?>()
        var directMatches = 0
        var catalogMatches = 0
        var attempted = 0
        var matched = 0
        var fetched = 0
        var unsuitable = 0
        var failed = 0
        var saved = 0
        for (rom in pending) {
            if (!shouldContinue() || attempted >= MAX_ATTEMPTS ||
                saved >= MAX_SAVED) break
            val target = CartLabelPaths.file(context.filesDir, rom.uri)
            if (target.exists()) continue
            attempted++
            var outcome = "NOT_FOUND"
            try {
                // Preserve the exact checksum match as the highest priority.
                val crc = CartStudioCatalog.cached(context, rom)
                    ?: CartStudioCatalog.identify(context, rom)
                if (!shouldContinue()) break
                val direct = OfficialArtResolver.artPath(index, rom.platform, crc.hex)
                val dat = if (direct == null && shouldContinue()) {
                    dats.getOrPut(rom.platform) {
                        OfficialDatSource.load(context, rom.platform, shouldContinue)
                    }
                } else null
                if (direct == null && dat == null) outcome = "DAT_UNAVAILABLE"
                val resolved = OfficialArtResolver.match(rom, crc, index, dat)
                val path = resolved?.let {
                    OfficialArtResolver.artPath(index, rom.platform, it)
                } ?: continue
                if (direct != null) directMatches++ else catalogMatches++
                matched++
                outcome = "MATCHED"
                val response = art.intercept(path)
                if (response.statusCode != 200) {
                    outcome = if (response.statusCode >= 500 ||
                        response.statusCode == 429) "NETWORK_ERROR" else "NOT_FOUND"
                    failed++
                    continue
                }
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
                if (!shouldContinue()) break
                fetched++
                if (bytes.size !in 8..MAX_SOURCE ||
                    !bytes.copyOfRange(0, 8).contentEquals(pngSignature)) {
                    outcome = "INVALID_ARTWORK"
                    unsuitable++
                    continue
                }
                val label = cropPhysicalCartLabel(bytes, rom.platform)
                if (label == null) {
                    outcome = "INVALID_ARTWORK"
                    unsuitable++
                    continue
                }
                if (label.size > MAX_LABEL || target.exists()) {
                    outcome = if (label.size > MAX_LABEL) "INVALID_ARTWORK" else "EXISTS"
                    unsuitable++
                    continue
                }
                if (!shouldContinue()) break
                target.parentFile?.mkdirs()
                // Atomic, never replace a manual or Cart Studio label.
                if (target.exists()) continue
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
                failed++
                Log.w(TAG, "Skipping unsupported/failed cart art", error)
            } finally {
                // Exact v1.2.0 scan lifecycle. Metrics never affect retries.
                if (shouldContinue()) examinedThisSession.add(rom.uri)
                AutoCartLabelDiagnostics.result(examinedThisSession.size, savedTotal, outcome)
                // The game's library TITLE is sufficient for user-side
                // diagnostics; never log SAF URI, save data or hashes.
                Log.i(TAG, "ART_RESULT $outcome [${rom.platform}] " +
                    rom.title.take(96))
            }
        }
        try { CartStudioCatalog.flush(context) }
        catch (error: Exception) { Log.w(TAG, "Could not persist cartridge checksums", error) }

        val remaining = missing.count { it.uri !in examinedThisSession &&
            !CartLabelPaths.file(context.filesDir, it.uri).exists() }
        Log.i(TAG, "Art pass complete: checked=$attempted, indexedMatches=$matched, " +
            "direct=$directMatches, dat=$catalogMatches, " +
            "downloaded=$fetched, rejected=$unsuitable, failed=$failed, saved=$saved, " +
            "remaining=$remaining, totalSaved=$savedTotal")
        // Try the next 16 candidates while foregrounded, never hammering the
        // first 16 ROMs or exceeding eight saved labels per batch.
        return shouldContinue() && remaining > 0
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
