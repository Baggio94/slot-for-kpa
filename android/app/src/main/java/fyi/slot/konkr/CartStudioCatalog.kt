package fyi.slot.konkr

import android.content.Context
import android.net.Uri
import android.util.AtomicFile
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.InputStream
import java.util.Locale
import java.util.zip.CRC32

/**
 * Android Storage Access Framework adapter for the *official* Slot Cart Studio.
 *
 * Content URIs are read-only, never turned into physical paths or modified.
 * This is the identity/cache foundation; artwork retrieval and the original
 * Studio WebView editor will be wired in subsequent dev26 steps.
 */
internal object CartStudioCatalog {
    private const val LIBRARY = "rom-library-cache-v1.json"
    private const val CACHE = "cart-studio-crc-cache-v1.json"
    private const val MAX_LIBRARY_BYTES = 8L * 1024 * 1024
    private const val HEAD_BYTES = 0x150

    internal data class Rom(
        val uri: String,
        val platform: String,
        val title: String,
        val size: Long,
        val modified: Long,
    )

    internal data class Fingerprint(
        val crc: Long,
        val head: ByteArray,
    ) {
        val hex: String get() = java.lang.Long.toHexString(crc)
            .uppercase(Locale.ROOT).padStart(8, '0')
    }

    /** Use the same ROM index already accepted by nativeSetLibrary. */
    fun games(context: Context): List<Rom> {
        val file = File(context.filesDir, LIBRARY)
        if (!file.isFile || file.length() !in 1..MAX_LIBRARY_BYTES) return emptyList()
        val root = JSONObject(file.readText(Charsets.UTF_8))
        if (root.optInt("schema") != 1) return emptyList()
        val entries = root.optJSONArray("games") ?: return emptyList()
        val result = ArrayList<Rom>(entries.length())
        for (i in 0 until entries.length().coerceAtMost(5000)) {
            val row = entries.optJSONObject(i) ?: continue
            val uri = row.optString("uri")
            val platform = row.optString("platform")
            if (!uri.startsWith("content://") || platform !in setOf("GBA", "GB", "GBC")) continue
            result.add(Rom(uri, platform, row.optString("title"),
                row.optLong("size", -1), row.optLong("modified", -1)))
        }
        return result
    }

    /** Layer local per-ROM Studio shell choices over the existing Android ROM index. */
    fun withOverrides(context: Context, source: String): String {
        val entries = JSONArray(source)
        for (i in 0 until entries.length()) {
            val game = entries.optJSONObject(i) ?: continue
            val uri = game.optString("uri")
            if (!uri.startsWith("content://")) continue
            CartStudioBridge.savedShell(context, uri)?.let {
                game.put("shell_override", it)
            }
        }
        return entries.toString()
    }

    /**
     * Official Cart Studio matching starts with CRC32 of uncompressed ROM bytes,
     * and reads 0x150 bytes for the original cart shell detection. Both are
     * computed in one streaming pass, without loading a 32 MiB ROM into RAM.
     */
    fun fingerprintStream(input: InputStream): Fingerprint {
        val crc = CRC32()
        val head = ByteArray(HEAD_BYTES)
        var got = 0
        val buffer = ByteArray(32768)
        input.use { stream ->
            while (true) {
                val n = stream.read(buffer)
                if (n < 0) break
                if (n == 0) continue
                crc.update(buffer, 0, n)
                if (got < HEAD_BYTES) {
                    val take = minOf(n, HEAD_BYTES - got)
                    System.arraycopy(buffer, 0, head, got, take)
                    got += take
                }
            }
        }
        return Fingerprint(crc.value, head.copyOf(got))
    }

    // A full Studio catalog can contain hundreds of carts. The old adapter
    // decoded and rewrote the complete JSON file once PER cartridge (quadratic
    // disk overhead). Keep one in-memory index and commit misses in batches.
    private var memoryPath: String? = null
    private var memoryRecords: JSONObject? = null
    private var unsavedFingerprints = 0
    private const val CACHE_BATCH = 12

    private fun records(context: Context): JSONObject {
        val path = File(context.filesDir, CACHE).absolutePath
        if (memoryPath == path && memoryRecords != null) return memoryRecords!!
        val doc = try { JSONObject(File(path).readText(Charsets.UTF_8)) }
                  catch (_: Exception) { JSONObject() }
        val entries = if (doc.optInt("schema") == 1)
            doc.optJSONObject("records") ?: JSONObject()
        else JSONObject()
        memoryPath = path
        memoryRecords = entries
        unsavedFingerprints = 0
        return entries
    }

    @Synchronized
    fun flush(context: Context) {
        if (unsavedFingerprints == 0) return
        val entries = records(context)
        val cacheFile = File(context.filesDir, CACHE)
        val atomic = AtomicFile(cacheFile)
        val output = atomic.startWrite()
        try {
            output.write(JSONObject().put("schema", 1)
                .put("records", entries).toString().toByteArray(Charsets.UTF_8))
            atomic.finishWrite(output)
            unsavedFingerprints = 0
        } catch (failure: Exception) {
            atomic.failWrite(output)
            throw failure
        }
    }

    /**
     * Look up verified fingerprints without touching ROM bytes. Every cached
     * record is tied to size AND modification time; unknown SAF metadata
     * forces the original streaming CRC path. Used during the first session
     * bridge call to eliminate repeated WebView/Java boundary traffic.
     */
    @Synchronized
    fun cached(context: Context, rom: Rom): Fingerprint? {
        if (rom.size <= 0 || rom.modified <= 0) return null
        val old = records(context).optJSONObject(rom.uri) ?: return null
        if (old.optLong("size") != rom.size ||
            old.optLong("modified") != rom.modified) return null
        return try {
            val crc = old.getLong("crc")
            val header = Base64.decode(old.getString("head"), Base64.DEFAULT)
            if (crc in 0..0xFFFFFFFFL && header.size == HEAD_BYTES)
                Fingerprint(crc, header)
            else null
        } catch (_: Exception) { null }
    }

    /**
     * Called off the UI thread. Missing or unreliable SAF metadata triggers
     * recalculation, while unchanged ROMs reuse the exact previous CRC/header.
     * The partial batch is flushed on CartStudioActivity.onDestroy().
     */
    @Synchronized
    fun identify(context: Context, rom: Rom): Fingerprint {
        require(rom.uri.startsWith("content://")) { "A SAF ROM URI is required" }
        cached(context, rom)?.let { return it }
        val entries = records(context)
        val content = context.contentResolver.openInputStream(Uri.parse(rom.uri))
            ?: throw IllegalArgumentException("Unable to read the ROM")
        val fingerprint = fingerprintStream(content)
        if (rom.size > 0 && rom.modified > 0 && fingerprint.head.size == HEAD_BYTES) {
            entries.put(rom.uri, JSONObject()
                .put("size", rom.size)
                .put("modified", rom.modified)
                .put("crc", fingerprint.crc)
                .put("head", Base64.encodeToString(fingerprint.head, Base64.NO_WRAP)))
            unsavedFingerprints++
            if (unsavedFingerprints >= CACHE_BATCH) flush(context)
        }
        return fingerprint
    }
}
