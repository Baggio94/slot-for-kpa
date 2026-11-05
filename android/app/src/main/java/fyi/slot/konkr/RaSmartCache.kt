package fyi.slot.konkr

import android.content.Context
import android.net.Uri
import android.util.AtomicFile
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest

/**
 * Background SAF index for GBA, GB, and GBC RetroAchievements prefetch.
 *
 * This never duplicates ROM files, stores credentials, or changes saves/states.
 * Full uncompressed ROM MD5s match the existing native rcheevos GBA contract.
 * Native Rust alone owns RA HTTPS, the game definitions and the award ledger.
 */
internal object RaSmartCache {
    private const val SCHEMA = 1
    private const val MAX_LIBRARY_BYTES = 8 * 1024 * 1024L
    private const val MAX_ROMS = 5000
    private const val MAX_ROM_BYTES = 64L * 1024 * 1024
    private const val INDEX_FILE = "ra-smart-rom-index-v1.json"

    internal data class Fingerprint(val size: Long, val modified: Long, val md5: String)

    internal fun canReuse(cached: Fingerprint?, size: Long, modified: Long): Boolean =
        cached != null && size > 0 && modified > 0 &&
            cached.size == size && cached.modified == modified &&
            cached.md5.length == 32 && cached.md5.all { it in '0'..'9' || it in 'a'..'f' }

    private fun hashRom(context: Context, uri: Uri, filename: String, console: Int): String {
        val digest = MessageDigest.getInstance("MD5") // RA's ROM identity, not for security
        // RetroAchievements hashes NES carts without their iNES header and Super
        // Nintendo carts without a 512-byte copier header, as the native rcheevos
        // hash at launch does; every other cart whole. Whether a Super Nintendo
        // dump carries a copier header is only known from its unpacked size, so
        // that cart is hashed both ways and the right one kept at the end.
        val copier = if (console == 3) MessageDigest.getInstance("MD5") else null
        var consumed = 0L
        context.contentResolver.openInputStream(uri)?.use { source ->
            RomSource.useRom(source, filename) { input, _ ->
                var skip = 0
                val buffer = ByteArray(64 * 1024)
                var first = true
                while (true) {
                    if (Thread.currentThread().isInterrupted) throw InterruptedException()
                    val length = input.read(buffer)
                    if (length < 0) break
                    if (first && length > 0) {
                        first = false
                        if (console == 7 && length > 16 && buffer[0] == 'N'.code.toByte() &&
                            buffer[1] == 'E'.code.toByte() && buffer[2] == 'S'.code.toByte() &&
                            buffer[3] == 0x1a.toByte()) skip = 16
                    }
                    if (copier != null) {
                        val drop = (512 - consumed).coerceIn(0, length.toLong()).toInt()
                        if (length > drop) copier.update(buffer, drop, length - drop)
                    }
                    consumed += length
                    require(consumed <= MAX_ROM_BYTES) { "ROM is too large" }
                    val from = minOf(skip, length)
                    skip -= from
                    if (length > from) digest.update(buffer, from, length - from)
                }
            }
        } ?: error("Cannot open ROM through Android SAF")
        require(consumed >= 0x150) { "ROM is too small" }
        val chosen = if (copier != null && consumed % 1024 == 512L) copier else digest
        return chosen.digest().joinToString("") { "%02x".format(it.toInt() and 0xff) }
    }

    private fun loadIndex(root: File, tree: String): Map<String, Fingerprint> {
        val file = File(root, INDEX_FILE)
        if (!file.isFile || file.length() !in 1L..MAX_LIBRARY_BYTES) return emptyMap()
        return try {
            val json = JSONObject(file.readText(Charsets.UTF_8))
            if (json.optInt("schema") != SCHEMA || json.optString("root") != tree)
                return emptyMap()
            val entries = json.optJSONArray("entries") ?: return emptyMap()
            val result = HashMap<String, Fingerprint>()
            for (i in 0 until entries.length().coerceAtMost(MAX_ROMS)) {
                val item = entries.optJSONObject(i) ?: continue
                val uri = item.optString("uri")
                if (!uri.startsWith("content://")) continue
                result[uri] = Fingerprint(item.optLong("size", -1),
                    item.optLong("modified", -1), item.optString("md5"))
            }
            result
        } catch (_: Exception) {
            emptyMap()
        }
    }

    private fun persistIndex(root: File, tree: String, entries: Map<String, Fingerprint>) {
        val array = JSONArray()
        for ((uri, value) in entries) {
            array.put(JSONObject().put("uri", uri).put("size", value.size)
                .put("modified", value.modified).put("md5", value.md5))
        }
        val data = JSONObject().put("schema", SCHEMA).put("root", tree)
            .put("entries", array).toString().toByteArray(Charsets.UTF_8)
        if (data.size > MAX_LIBRARY_BYTES) return
        val file = AtomicFile(File(root, INDEX_FILE))
        val out = file.startWrite()
        try {
            out.write(data)
            file.finishWrite(out)
        } catch (error: Exception) {
            file.failWrite(out)
            throw error
        }
    }

    /**
     * Execute ONLY from a low-priority background executor after the app has
     * opted in to RetroAchievements. The caller owns lifecycle/cancellation.
     */
    fun indexAndPrefetch(
        context: Context,
        root: File,
        isPlaying: () -> Boolean,
        awaitForeground: () -> Unit,
        submit: (String, Int) -> Unit,
    ) {
        val shelf = File(root, "rom-library-cache-v1.json")
        if (!shelf.isFile || shelf.length() !in 1L..MAX_LIBRARY_BYTES) return
        val data = JSONObject(shelf.readText(Charsets.UTF_8))
        if (data.optInt("schema") != 1) return
        val tree = data.optString("root")
        val entries = data.optJSONArray("games") ?: return
        val existing = loadIndex(root, tree)
        val updated = HashMap(existing)
        val submitted = HashSet<String>()
        var fresh = 0
        for (i in 0 until entries.length().coerceAtMost(MAX_ROMS)) {
            if (Thread.currentThread().isInterrupted) break
            // Suspend SAF fingerprinting without polling once the screen is off.
            awaitForeground()
            val item = entries.optJSONObject(i) ?: continue
            val console = when (item.optString("platform")) {
                "GB" -> 4
                "GBA" -> 5
                "GBC" -> 6
                Systems.NES -> 7
                Systems.SNES -> 3
                Systems.MD -> 1
                else -> continue
            }
            val uriText = item.optString("uri")
            if (!uriText.startsWith("content://")) continue
            val size = item.optLong("size", -1)
            val modified = item.optLong("modified", -1)
            try {
                val hash = existing[uriText]?.takeIf {
                    canReuse(it, size, modified)
                }?.md5 ?: hashRom(context, Uri.parse(uriText),
                    item.optString("filename").ifBlank {
                        Uri.parse(uriText).lastPathSegment.orEmpty()
                    }, console).also {
                    updated[uriText] = Fingerprint(size, modified, it)
                    fresh++
                }
                if (submitted.add(hash)) {
                    // Never block emulation startup or enqueue messages while
                    // the Activity's GLSurfaceView is paused in the background.
                    while (isPlaying()) {
                        awaitForeground()
                        if (Thread.currentThread().isInterrupted) return
                        Thread.sleep(1200)
                    }
                    awaitForeground()
                    submit(hash, console)
                    Thread.sleep(60) // prevent filling GL input during a 5k-game scan
                }
                if (fresh > 0 && fresh % 25 == 0) {
                    persistIndex(root, tree, updated)
                    fresh = 0
                }
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                break
            } catch (_: Exception) {
                // Individual unreadable ROMs must not prevent other games from
                // being prepared; no URI, file content or credentials in logs.
            }
        }
        if (!Thread.currentThread().isInterrupted) persistIndex(root, tree, updated)
    }
}
