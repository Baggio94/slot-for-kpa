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
import java.security.MessageDigest

/**
 * CORS-safe, allowlisted art source for the *original* Cart Studio WebView.
 *
 * In a normal browser the Studio runs on studio.slot-cfw.fyi. Inside Slot the
 * origin is appassets.androidplatform.net, which is NOT necessarily admitted
 * by the art CDN's CORS policy. Serve the identical upstream bytes from our
 * appassets origin: /studio/art/index.json and /studio/art/<platform>/<art>.png.
 *
 * We neither scrape HTML nor modify upstream artwork. A bounded private disk
 * cache makes previously fetched art available after network interruptions.
 * No arbitrary URL, path traversal, redirects to foreign hosts or ROM writes.
 */
internal class CartStudioArtProxy(private val context: Context) {
    companion object {
        private const val TAG = "SlotStudioArt"
        private const val HOST = "art.slot-cfw.fyi"
        private const val BASE = "https://art.slot-cfw.fyi/"
        private const val MAX_INDEX_BYTES = 64 * 1024 * 1024
        private const val MAX_IMAGE_BYTES = 8 * 1024 * 1024
        private const val FRESH_MS = 7L * 24 * 60 * 60 * 1000
        private val safePath = Regex("[a-zA-Z0-9_./-]+")
    }

    private val cacheDir = File(context.cacheDir, "slot-studio-art").apply { mkdirs() }

    private fun response(status: Int, type: String, bytes: ByteArray): WebResourceResponse =
        WebResourceResponse(type, if (type == "application/json") "UTF-8" else null,
            status, when (status) {
                200 -> "OK"
                404 -> "Not Found"
                503 -> "Service Unavailable"
                else -> "Bad Gateway"
            }, mapOf("Cache-Control" to "no-store"),
            ByteArrayInputStream(bytes))

    fun intercept(path: String): WebResourceResponse {
        val allowed = safePath.matches(path) &&
            !path.startsWith("/") &&
            !path.split('/').contains("..") &&
            (path == "index.json" || path.endsWith(".png", ignoreCase = true))
        if (!allowed) return response(404, "text/plain", "Not found".toByteArray())

        val isIndex = path == "index.json"
        val type = if (isIndex) "application/json" else "image/png"
        val limit = if (isIndex) MAX_INDEX_BYTES else MAX_IMAGE_BYTES
        val digest = MessageDigest.getInstance("SHA-256")
            .digest(path.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it.toInt() and 0xff) }
        val cache = File(cacheDir, digest + if (isIndex) ".json" else ".png")
        fun previous(): ByteArray? =
            if (cache.isFile && cache.length() in 1..limit.toLong())
                try { cache.readBytes() } catch (_: Exception) { null }
            else null

        if (cache.isFile && System.currentTimeMillis() - cache.lastModified() < FRESH_MS) {
            previous()?.let { return response(200, type, it) }
        }
        try {
            val connection = (URL(BASE + path).openConnection() as HttpURLConnection).apply {
                connectTimeout = 9_000
                readTimeout = 12_000
                requestMethod = "GET"
                instanceFollowRedirects = false
                setRequestProperty("Accept", if (isIndex) "application/json" else "image/png")
            }
            try {
                val status = connection.responseCode
                if (status == 404) return response(404, "text/plain", "Not found".toByteArray())
                check(status == 200) { "Artwork CDN returned HTTP $status" }
                val out = ByteArrayOutputStream()
                connection.inputStream.use { input ->
                    val buffer = ByteArray(32 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        if (count == 0) continue
                        require(out.size() + count <= limit) { "Artwork response too large" }
                        out.write(buffer, 0, count)
                    }
                }
                val bytes = out.toByteArray()
                require(bytes.isNotEmpty()) { "Empty artwork response" }
                if (isIndex) {
                    require(bytes[0] == '{'.code.toByte() || bytes[0] == '['.code.toByte()) {
                        "Art index is not JSON"
                    }
                } else {
                    require(bytes.size >= 8 && bytes.copyOfRange(0, 8).contentEquals(
                        byteArrayOf(-119, 80, 78, 71, 13, 10, 26, 10))) { "Art response is not PNG" }
                }
                try {
                    val atomic = AtomicFile(cache)
                    val stream = atomic.startWrite()
                    try {
                        stream.write(bytes)
                        atomic.finishWrite(stream)
                    } catch (error: Exception) {
                        atomic.failWrite(stream)
                        throw error
                    }
                } catch (error: Exception) {
                    Log.w(TAG, "Could not cache $path", error)
                }
                return response(200, type, bytes)
            } finally {
                connection.disconnect()
            }
        } catch (error: Exception) {
            Log.w(TAG, "Original art server request failed: $path", error)
            // A stale but previously verified original file is safer than
            // silently substituting unrelated box art or a fabricated label.
            previous()?.let { return response(200, type, it) }
            return response(503, "text/plain",
                ("Official art server unavailable (" + error.javaClass.simpleName +
                    "). Check connection or retry later.").toByteArray())
        }
    }
}
