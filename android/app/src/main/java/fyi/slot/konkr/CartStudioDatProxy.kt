package fyi.slot.konkr

import android.content.Context
import android.util.AtomicFile
import android.util.Log
import android.webkit.WebResourceResponse
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/** Libretro's original No-Intro DAT text, cached locally for the Android WebView.
 * Only three hardcoded official URLs can be fetched. No arbitrary GitHub proxy. */
internal class CartStudioDatProxy(context: Context) {
    companion object {
        private const val TAG = "SlotStudioDat"
        private const val LIMIT = 16 * 1024 * 1024
        private const val FRESH_MS = 30L * 24 * 60 * 60 * 1000
        private const val ROOT =
            "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/"
        private val names = mapOf(
            "GBA" to "Nintendo%20-%20Game%20Boy%20Advance.dat",
            "GBC" to "Nintendo%20-%20Game%20Boy%20Color.dat",
            "GB" to "Nintendo%20-%20Game%20Boy.dat"
        )
    }
    private val dir = File(context.cacheDir, "slot-studio-dat").apply { mkdirs() }

    fun intercept(platform: String): WebResourceResponse {
        val fileName = names[platform]
            ?: return response(404, "Unknown platform")
        val cache = File(dir, "$platform.dat")
        fun cached(): ByteArray? = try {
            if (cache.isFile && cache.length() in 1..LIMIT.toLong())
                cache.readBytes() else null
        } catch (_: Exception) { null }
        if (System.currentTimeMillis() - cache.lastModified() < FRESH_MS) {
            cached()?.let { return response(200, bytes = it) }
        }
        try {
            val connection = (URL(ROOT + fileName).openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                instanceFollowRedirects = false
                connectTimeout = 8_000
                readTimeout = 15_000
            }
            val bytes = try {
                check(connection.responseCode == 200) {
                    "Libretro DAT returned HTTP ${connection.responseCode}"
                }
                val out = ByteArrayOutputStream()
                connection.inputStream.use { input ->
                    val buffer = ByteArray(32 * 1024)
                    while (true) {
                        val n = input.read(buffer)
                        if (n < 0) break
                        require(out.size() + n <= LIMIT) { "DAT exceeds 16 MiB" }
                        out.write(buffer, 0, n)
                    }
                }
                out.toByteArray()
            } finally { connection.disconnect() }
            require(bytes.isNotEmpty()) { "Empty DAT" }
            // No-Intro text always contains ROM entries.
            require(String(bytes, 0, minOf(4096, bytes.size), Charsets.UTF_8)
                .contains("game", ignoreCase = true)) { "Invalid DAT response" }
            try {
                val atomic = AtomicFile(cache)
                val output = atomic.startWrite()
                try { output.write(bytes); atomic.finishWrite(output) }
                catch (e: Exception) { atomic.failWrite(output); throw e }
            } catch (e: Exception) {
                Log.w(TAG, "DAT cache write failed", e)
            }
            return response(200, bytes = bytes)
        } catch (error: Exception) {
            Log.w(TAG, "Libretro DAT unavailable: $platform", error)
            cached()?.let { return response(200, bytes = it) }
            return response(503, "Libretro DAT unavailable; retry with Wi-Fi enabled")
        }
    }

    private fun response(status: Int, message: String = "", bytes: ByteArray? = null) =
        WebResourceResponse("text/plain", "UTF-8", status,
            when (status) { 200 -> "OK"; 404 -> "Not Found"; else -> "Service Unavailable" },
            mapOf("Cache-Control" to "no-store"),
            ByteArrayInputStream(bytes ?: message.toByteArray(Charsets.UTF_8)))
}
