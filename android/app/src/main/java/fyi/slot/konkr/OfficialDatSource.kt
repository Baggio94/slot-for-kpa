package fyi.slot.konkr

import android.content.Context
import android.util.AtomicFile
import android.util.Log
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * The same three official Libretro No-Intro DATs that original Cart Studio
 * requests. Only these fixed HTTPS GitHub paths are permitted, and only on
 * the existing foreground-only, low-priority auto-art worker.
 */
internal object OfficialDatSource {
    private const val TAG = "SlotAutoLabels"
    private const val BASE = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/"
    private const val MAX_BYTES = 5 * 1024 * 1024
    private const val FRESH_MS = 7L * 24 * 60 * 60 * 1000
    private val names = mapOf(
        "GB" to "Nintendo - Game Boy.dat",
        "GBC" to "Nintendo - Game Boy Color.dat",
        "GBA" to "Nintendo - Game Boy Advance.dat",
    )

    fun load(context: Context, platform: String, shouldContinue: () -> Boolean):
        OfficialArtMatcher.Catalog? {
        val fileName = names[platform] ?: return null
        val cacheDir = File(context.cacheDir, "slot-studio-dat").apply { mkdirs() }
        val cache = File(cacheDir, "$platform.dat")

        fun cached(): String? = if (cache.isFile && cache.length() in 1..MAX_BYTES.toLong())
            runCatching { cache.readText(Charsets.UTF_8) }
                .getOrNull()?.takeIf { it.startsWith("clrmamepro (") }
        else null

        var dat = if (cache.isFile &&
            System.currentTimeMillis() - cache.lastModified() < FRESH_MS) cached() else null

        if (dat == null && shouldContinue()) {
            // URL path is a fixed allowlist, not user input.
            val url = URL(BASE + fileName.replace(" ", "%20"))
            try {
                val connection = (url.openConnection() as HttpURLConnection).apply {
                    connectTimeout = 9_000
                    readTimeout = 12_000
                    requestMethod = "GET"
                    instanceFollowRedirects = false
                    setRequestProperty("Accept", "text/plain")
                }
                try {
                    check(connection.responseCode == 200) { "No-Intro DAT HTTP ${connection.responseCode}" }
                    val out = ByteArrayOutputStream()
                    connection.inputStream.use { input ->
                        val buf = ByteArray(16 * 1024)
                        while (shouldContinue()) {
                            val n = input.read(buf)
                            if (n < 0) break
                            require(out.size() + n <= MAX_BYTES) { "No-Intro DAT exceeds size cap" }
                            out.write(buf, 0, n)
                        }
                        check(shouldContinue()) { "Auto-art stopped" }
                    }
                    val downloaded = out.toString(Charsets.UTF_8.name())
                    require(downloaded.startsWith("clrmamepro (") &&
                        downloaded.contains("\ngame (")) { "Malformed No-Intro DAT" }
                    dat = downloaded
                    val atomic = AtomicFile(cache)
                    val stream = atomic.startWrite()
                    try {
                        stream.write(out.toByteArray())
                        atomic.finishWrite(stream)
                    } catch (error: Exception) {
                        atomic.failWrite(stream)
                        Log.w(TAG, "No-Intro cache not writable", error)
                    }
                } finally {
                    connection.disconnect()
                }
            } catch (error: Exception) {
                Log.w(TAG, "No-Intro lookup deferred for $platform", error)
            }
        }
        // A verified stale local database is still usable offline.
        val text = dat ?: cached() ?: return null
        return runCatching { OfficialArtMatcher.Catalog.parse(text, platform) }
            .onFailure { Log.w(TAG, "Invalid No-Intro database $platform", it) }
            .getOrNull()
    }
}
