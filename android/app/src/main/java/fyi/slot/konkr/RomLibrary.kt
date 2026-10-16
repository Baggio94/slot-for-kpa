package fyi.slot.konkr

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import org.json.JSONArray
import org.json.JSONObject
import java.util.ArrayDeque
import java.util.HashSet
import java.util.Locale

/**
 * Read-only ROM discovery using the Storage Access Framework.
 * Only file names, small cartridge headers and opaque content URIs are read.
 * No commercial games, BIOS files, or writable access are requested or copied.
 */
internal object RomLibrary {
    private const val MAX_ROMS = 5000
    private const val MAX_DIRECTORIES = 1500
    private const val MAX_DEPTH = 12

    internal data class ScanResult(val json: String, val count: Int, val truncated: Boolean)
    /** Ignore SAF iteration order, but never hide real additions/removals. */
    internal data class IndexEntry(
        val uri: String, val title: String, val filename: String, val platform: String,
        val code: String, val colorOnly: Boolean, val size: Long, val modified: Long
    )
    internal fun equivalentEntries(left: List<IndexEntry>, right: List<IndexEntry>): Boolean {
        if (left.size != right.size) return false
        val a = left.associateBy { it.uri }
        val b = right.associateBy { it.uri }
        return a.size == left.size && b.size == right.size && a == b
    }
    internal fun indexEntries(json: String): List<IndexEntry> {
        val arr = JSONArray(json)
        require(arr.length() <= MAX_ROMS) { "ROM index exceeds bound" }
        return (0 until arr.length()).map { i ->
            val obj = arr.getJSONObject(i)
            IndexEntry(obj.getString("uri"), obj.optString("title"),
                obj.optString("filename"), obj.optString("platform"),
                obj.optString("code"), obj.optBoolean("color_only"),
                obj.optLong("size", -1), obj.optLong("modified", -1))
        }
    }
    internal fun sameIndex(before: String, after: String): Boolean =
        equivalentEntries(indexEntries(before), indexEntries(after))
    private data class Directory(val id: String, val depth: Int, val folder: String)

    /** Regardless of root folder layout, only GB/GBC/GBA ROM files are indexed. */
    internal fun isSupportedRomExtension(extension: String): Boolean =
        extension.lowercase(Locale.ROOT) in setOf("gba", "gb", "gbc")

    /**
     * Re-scan the selected tree, but avoid re-opening every ROM header when
     * its exact SAF URI, display name, size and last-modified time match our
     * previously persisted library index. Unknown provider metadata still
     * forces a real header read (never trust size=0 or modified=0).
     */
    fun scan(context: Context, treeUri: Uri, oldGamesJson: String? = null): ScanResult {
        val previous = HashMap<String, JSONObject>()
        val cachedJson = oldGamesJson ?: try {
            val cache = java.io.File(context.filesDir, "rom-library-cache-v1.json")
            if (cache.length() > 8L * 1024 * 1024) null
            else JSONObject(cache.readText(Charsets.UTF_8)).let { meta ->
                if (meta.optInt("schema") == 1 &&
                    meta.optString("root") == treeUri.toString())
                    meta.optJSONArray("games")?.toString()
                else null
            }
        } catch (_: Exception) { null }
        if (cachedJson != null) {
            try {
                val cached = JSONArray(cachedJson)
                for (index in 0 until cached.length().coerceAtMost(MAX_ROMS)) {
                    val entry = cached.optJSONObject(index) ?: continue
                    val uri = entry.optString("uri")
                    if (uri.startsWith("content://")) previous[uri] = entry
                }
            } catch (_: Exception) {
                // Bad cache affects speed only; discovery still reads headers.
            }
        }
        var headersRead = 0
        var headersReused = 0
        val resolver = context.contentResolver
        val rootId = DocumentsContract.getTreeDocumentId(treeUri)
        val pending = ArrayDeque<Directory>()
        val seen = HashSet<String>()
        // Search the entire user-selected SAF tree, including custom layouts.
        // Do not pre-enumerate the root or assume platform-named folders.
        pending.add(Directory(rootId, 0, ""))
        android.util.Log.i("SlotKonkr", "ROM scan scope: full selected tree")
        val games = JSONArray()
        var directories = 0
        var truncated = false
        val columns = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_SIZE,
            DocumentsContract.Document.COLUMN_LAST_MODIFIED
        )

        while (pending.isNotEmpty()) {
            if (directories >= MAX_DIRECTORIES || games.length() >= MAX_ROMS) {
                truncated = true
                break
            }
            val dir = pending.removeFirst()
            if (!seen.add(dir.id)) continue
            directories++
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, dir.id)
            // Null means provider unavailable, not an empty ROM folder.
            val listing = resolver.query(children, columns, null, null, null)
                ?: throw java.io.IOException("SAF folder listing unavailable")
            listing.use { cursor ->
                val idIndex = cursor.getColumnIndexOrThrow(columns[0])
                val nameIndex = cursor.getColumnIndexOrThrow(columns[1])
                val mimeIndex = cursor.getColumnIndexOrThrow(columns[2])
                // Cloud/document providers may omit these optional columns.
                val sizeIndex = cursor.getColumnIndex(columns[3])
                val modifiedIndex = cursor.getColumnIndex(columns[4])
                while (cursor.moveToNext()) {
                    val id = cursor.getString(idIndex) ?: continue
                    val name = cursor.getString(nameIndex) ?: continue
                    val mime = cursor.getString(mimeIndex) ?: continue
                    if (name.startsWith(".")) continue
                    if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                        if (dir.depth < MAX_DEPTH && !ignoredDirectory(name)) {
                            pending.add(Directory(id, dir.depth + 1, name))
                        }
                        continue
                    }

                    val extension = name.substringAfterLast('.', "").lowercase(Locale.ROOT)
                    if (!isSupportedRomExtension(extension)) continue
                    if (games.length() >= MAX_ROMS) {
                        truncated = true
                        break
                    }
                    val fileUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, id)
                    // Keep the shelf title identical to the RetroArch save basename.
                    val title = name.substringBeforeLast('.').trim().take(200)
                    if (title.isEmpty()) continue
                    val uriText = fileUri.toString()
                    val size = if (sizeIndex >= 0 && !cursor.isNull(sizeIndex))
                        cursor.getLong(sizeIndex) else -1L
                    val modified = if (modifiedIndex >= 0 && !cursor.isNull(modifiedIndex))
                        cursor.getLong(modifiedIndex) else -1L
                    val old = previous[uriText]
                    val reuse = old != null && canReuseHeader(
                        old.optString("title"), old.optLong("size", -1),
                        old.optLong("modified", -1), title, size, modified)
                    val gba = extension == "gba"
                    var code: String
                    var colorOnly: Boolean
                    var platform: String
                    if (reuse && old != null) {
                        headersReused++
                        code = old.optString("code")
                        colorOnly = old.optBoolean("color_only")
                        platform = old.optString("platform")
                    } else {
                        headersRead++
                        val header = cartridgeHeader(context, fileUri)
                        code = if (gba && header.size >= 0xb0) {
                            String(header, 0xac, 4, Charsets.US_ASCII)
                                .takeIf { it.all { c -> c in 'A'..'Z' || c in '0'..'9' } }
                                ?: ""
                        } else ""
                        colorOnly = !gba && (extension == "gbc" ||
                            (header.size > 0x143 && (header[0x143].toInt() and 0xff) == 0xc0))
                        val container = dir.folder.lowercase(Locale.ROOT)
                        val gbColor = !gba && (extension == "gbc" || colorOnly ||
                            container == "gbc" || container == "game boy color")
                        platform = when {
                            gba -> "GBA"
                            gbColor -> "GBC"
                            else -> "GB"
                        }
                    }
                    games.put(JSONObject()
                        .put("title", title)
                        .put("filename", name)
                        .put("platform", platform)
                        .put("uri", uriText)
                        .put("code", code)
                        .put("color_only", colorOnly)
                        .put("size", size)
                        .put("modified", modified))
                }
            }
        }
        android.util.Log.i("SlotKonkr", "ROM scan: $directories folders, ${games.length()} carts, " +
            "headers read=$headersRead, reused=$headersReused")
        return ScanResult(games.toString(), games.length(), truncated)
    }

    /** Only stable nonzero provider metadata permits skipping a header read. */
    internal fun canReuseHeader(oldTitle: String?, oldSize: Long, oldModified: Long,
                                title: String, size: Long, modified: Long): Boolean =
        oldTitle == title && size > 0 && modified > 0 &&
            oldSize == size && oldModified == modified

    private fun ignoredDirectory(name: String): Boolean {
        return name.lowercase(Locale.ROOT) in setOf(
            "bios", "saves", "savestates", "states", "screenshots", "videos",
            "themes", "media", "images", "artwork", "covers", "system volume information"
        )
    }

    private fun cartridgeHeader(context: Context, fileUri: Uri): ByteArray {
        return try {
            context.contentResolver.openInputStream(fileUri)?.use { stream ->
                val header = ByteArray(0x150)
                var n = 0
                while (n < header.size) {
                    val bytes = stream.read(header, n, header.size - n)
                    if (bytes <= 0) break
                    n += bytes
                }
                header.copyOf(n)
            } ?: byteArrayOf()
        } catch (_: Exception) {
            byteArrayOf()
        }
    }
}
