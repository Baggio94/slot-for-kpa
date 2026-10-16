package fyi.slot.konkr

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.AtomicFile
import android.util.Log
import org.json.JSONObject
import java.io.ByteArrayOutputStream

/**
 * Conservative automatic cartridge label acquisition. Reuses the official
 * Cart Studio checksum-indexed scans, so ambiguous filename matches never
 * put the wrong label on a game. Manual and Studio labels always win.
 *
 * Low priority and bounded: at most eight new labels per session, 16 attempts,
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

    fun fill(context: Context, shouldContinue: () -> Boolean, onSaved: (String) -> Unit) {
        if (!shouldContinue()) return
        val games = CartStudioCatalog.games(context)
        if (games.isEmpty()) return
        val missing = games.filter { !CartLabelPaths.file(context.filesDir, it.uri).exists() }
        if (missing.isEmpty()) return

        // Original Studio's existing HTTPS allowlist, disk cache and bounded
        // response handling. Avoid another artwork source or extra permissions.
        val indexResponse = CartStudioArtProxy(context).intercept("index.json")
        if (indexResponse.statusCode != 200) return
        val indexBytes = indexResponse.data.use { input ->
            val stream = ByteArrayOutputStream()
            val buffer = ByteArray(16 * 1024)
            while (true) {
                if (!shouldContinue()) return
                val n = input.read(buffer)
                if (n < 0) break
                if (stream.size() + n > MAX_INDEX) return
                stream.write(buffer, 0, n)
            }
            stream.toByteArray()
        }
        val index = try { JSONObject(String(indexBytes, Charsets.UTF_8)) }
            catch (_: Exception) { return }
        val art = CartStudioArtProxy(context)
        var attempted = 0
        var saved = 0
        for (rom in missing) {
            if (!shouldContinue() || attempted >= MAX_ATTEMPTS || saved >= MAX_SAVED) break
            val target = CartLabelPaths.file(context.filesDir, rom.uri)
            if (target.exists()) continue
            attempted++
            try {
                // Exact ROM checksum, never an unreliable filename-only match.
                val crc = CartStudioCatalog.cached(context, rom)
                    ?: CartStudioCatalog.identify(context, rom)
                if (!shouldContinue()) break
                val path = index.optJSONObject(crc.hex)
                    ?.optString("support-texture").orEmpty()
                if (!validArtworkPath(path, rom.platform)) continue
                val response = art.intercept(path)
                if (response.statusCode != 200) continue
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
                if (!shouldContinue() || bytes.size !in 8..MAX_SOURCE ||
                    !bytes.copyOfRange(0, 8).contentEquals(pngSignature)) continue
                val label = cropPhysicalCartLabel(bytes, rom.platform) ?: continue
                if (label.size > MAX_LABEL || target.exists() || !shouldContinue()) continue
                target.parentFile?.mkdirs()
                // Re-check immediately before atomic publication. Existing
                // custom artwork is never intentionally replaced.
                if (target.exists()) continue
                val atomic = AtomicFile(target)
                val stream = atomic.startWrite()
                try {
                    stream.write(label)
                    atomic.finishWrite(stream)
                } catch (e: Exception) {
                    atomic.failWrite(stream)
                    throw e
                }
                saved++
                onSaved(rom.uri)
                Thread.sleep(120L)
            } catch (error: Exception) {
                Log.w(TAG, "Skipping unsupported/failed cart art", error)
            }
        }
        CartStudioCatalog.flush(context)
        if (saved > 0) Log.i(TAG, "Prepared $saved official cartridge labels")
    }

    internal fun validArtworkPath(path: String, platform: String): Boolean =
        path.isNotBlank() && path.length <= 150 &&
            safeArtPath.matches(path) && !path.startsWith("/") &&
            !path.split('/').contains("..") && path.endsWith(".png") &&
            path.startsWith("$platform/")

    /**
     * The official Cart Studio real-scan layout. Unknown box art and angled
     * photos are refused rather than cropped into an invented cartridge label.
     * Coordinates follow the upstream fork's straight-on cart layout.
     */
    private fun cropPhysicalCartLabel(bytes: ByteArray, platform: String): ByteArray? {
        val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)
        val w = opts.outWidth
        val h = opts.outHeight
        if (w !in 300..3000 || h !in 280..3000) return null
        val aspect = w.toDouble() / h
        val gba = platform == "GBA"
        if (gba && aspect !in 1.65..1.80) return null
        if (!gba && aspect !in 0.84..0.93) return null
        val source = BitmapFactory.decodeByteArray(bytes, 0, bytes.size) ?: return null
        return try {
            val x0 = if (gba) w * 137 / 1000 else w * 135 / 1000
            val y0 = if (gba) h * 226 / 1000 else h * 285 / 1000
            val x1 = if (gba) w * 865 / 1000 else w * 865 / 1000
            val y1 = if (gba) h * 866 / 1000 else h * 858 / 1000
            val (tw, th) = if (gba) 264 to 136
                else if (platform == "GB") 220 to 195 else 223 to 199
            val crop = Bitmap.createBitmap(source, x0, y0, x1 - x0, y1 - y0)
            try {
                val result = Bitmap.createScaledBitmap(crop, tw, th, true)
                try {
                    val out = ByteArrayOutputStream()
                    if (!result.compress(Bitmap.CompressFormat.PNG, 100, out)) null
                    else out.toByteArray()
                } finally { result.recycle() }
            } finally { crop.recycle() }
        } finally { source.recycle() }
    }
}
