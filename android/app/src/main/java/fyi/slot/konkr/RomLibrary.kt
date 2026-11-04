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
        val code: String, val colorOnly: Boolean, val size: Long, val modified: Long,
        val gbTitle: String = "", val gbCode: String = "",
        val gbJapan: Boolean = false, val gbCgbFlag: Int = -1,
        val homeTitle: String = "", val homeRegion: String = "",
        val homeBattery: Boolean = false, val homeMaker: String = ""
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
                obj.optLong("size", -1), obj.optLong("modified", -1),
                obj.optString("gb_title"), obj.optString("gb_code"),
                obj.optBoolean("gb_japan"), obj.optInt("gb_cgb_flag", -1),
                obj.optString("home_title"), obj.optString("home_region"),
                obj.optBoolean("home_battery"), obj.optString("home_maker"))
        }
    }
    internal fun sameIndex(before: String, after: String): Boolean =
        equivalentEntries(indexEntries(before), indexEntries(after))
    private data class Directory(val id: String, val depth: Int, val folder: String)

    /** Every cartridge of Systems.ROM_EXTENSIONS, and ZIPs holding one. */
    internal fun isSupportedRomExtension(extension: String): Boolean =
        extension.lowercase(Locale.ROOT).let { it == "zip" || it in Systems.ROM_EXTENSIONS }

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
        // Search the user-selected SAF tree, including custom layouts, but
        // skip folders that ES-DE names after systems slot. does not run.
        pending.add(Directory(rootId, 0, ""))
        android.util.Log.i("SlotKonkr", "ROM scan scope: selected tree without other systems' folders")
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
                        old.optLong("modified", -1), title, size, modified) &&
                        // Old indexes lack the GB header metadata used by
                        // Cart Studio. Read only 0x150 bytes once to migrate.
                        // Older ones also lack whether a GB/GBA cart has a
                        // battery (2026-11): read once more to learn it.
                        (old.has("home_battery") && (old.optString("platform") == "GBA" ||
                            old.has("gb_cgb_flag") || old.has("home_maker")))
                    // ZIP archives remain a single shelf cartridge identified
                    // by the ZIP's SAF URI and filename. Resolve the embedded
                    // ROM header/extension only on cache misses.
                    val zipped = extension == "zip"
                    val zipHeader = if (zipped && !reuse) {
                        try {
                            resolver.openInputStream(fileUri)?.use { input ->
                                RomSource.inspect(input, name)
                            }
                        } catch (error: Exception) {
                            android.util.Log.w("SlotKonkr",
                                "Skipping unsupported/corrupt cartridge ZIP: " +
                                    name.take(80), error)
                            null
                        }
                    } else null
                    if (zipped && !reuse && zipHeader == null) continue
                    val embeddedExtension = zipHeader?.extension
                        ?: if (zipped) old?.optString("rom_ext")?.takeIf { it.isNotEmpty() }
                            ?: when (old?.optString("platform")) {
                            "GBA" -> "gba"
                            "GBC" -> "gbc"
                            else -> "gb"
                        } else extension
                    val gba = embeddedExtension == "gba"
                    val home = Systems.isHome(embeddedExtension)
                    var homeHeader = Systems.HomeHeader("", "", false)
                    var code: String
                    var colorOnly: Boolean
                    var platform: String
                    var gbTitle = ""
                    var gbCode = ""
                    var gbJapan = false
                    var gbCgbFlag = -1
                    if (reuse && old != null) {
                        headersReused++
                        code = old.optString("code")
                        colorOnly = old.optBoolean("color_only")
                        platform = old.optString("platform")
                        gbTitle = old.optString("gb_title")
                        gbCode = old.optString("gb_code")
                        gbJapan = old.optBoolean("gb_japan")
                        gbCgbFlag = old.optInt("gb_cgb_flag", -1)
                        homeHeader = Systems.HomeHeader(old.optString("home_title"),
                            old.optString("home_region"), old.optBoolean("home_battery"),
                            old.optString("home_maker"))
                    } else {
                        headersRead++
                        val header = zipHeader?.bytes
                            ?: cartridgeHeader(context, fileUri, Systems.headerBytes(embeddedExtension))
                        // A .bin is a Mega Drive cart only with a SEGA header.
                        if (!Systems.acceptsHeader(embeddedExtension, header)) continue
                        if (home) homeHeader = Systems.homeHeader(embeddedExtension, header, name)
                        else if (!gba) homeHeader = Systems.HomeHeader("", "", Systems.gbBattery(header))
                        else homeHeader = Systems.HomeHeader("", "", try {
                            resolver.openInputStream(fileUri)?.use { input ->
                                RomSource.useRom(input, name) { rom, _ -> Systems.gbaBattery(rom) }
                            } ?: false
                        } catch (_: Exception) { false })
                        // Use the exact Nintendo GB header fields read by
                        // Cart Studio's automatic shell selector. Never infer
                        // colours from the ROM filename or download artwork.
                        if (!gba && !home && header.size >= 0x14b) {
                            gbTitle = String(header, 0x134, 11, Charsets.US_ASCII)
                                .substringBefore('\u0000').trim()
                            gbCode = String(header, 0x13f, 4, Charsets.US_ASCII)
                                .takeIf { value -> value.all { it in 'A'..'Z' || it in '0'..'9' } }
                                ?: ""
                            gbJapan = header[0x14a].toInt() == 0
                            gbCgbFlag = header[0x143].toInt() and 0xff
                        }
                        code = if (gba && header.size >= 0xb0) {
                            String(header, 0xac, 4, Charsets.US_ASCII)
                                .takeIf { it.all { c -> c in 'A'..'Z' || c in '0'..'9' } }
                                ?: ""
                        } else ""
                        colorOnly = !gba && !home && (embeddedExtension == "gbc" ||
                            (header.size > 0x143 && (header[0x143].toInt() and 0xff) == 0xc0))
                        val container = dir.folder.lowercase(Locale.ROOT)
                        val gbColor = !gba && !home && (embeddedExtension == "gbc" || colorOnly ||
                            container == "gbc" || container == "game boy color")
                        platform = when {
                            home -> Systems.platformFor(embeddedExtension)!!
                            gba -> "GBA"
                            gbColor -> "GBC"
                            else -> "GB"
                        }
                    }
                    val game = JSONObject()
                        .put("title", title)
                        .put("filename", name)
                        .put("platform", platform)
                        .put("uri", uriText)
                        .put("code", code)
                        .put("color_only", colorOnly)
                        .put("size", size)
                        .put("modified", modified)
                    if (home) {
                        // The home console shells, labels and battery read
                        // these; rom_ext lets a cached ZIP keep its system.
                        game.put("rom_ext", embeddedExtension)
                            .put("home_title", homeHeader.title)
                            .put("home_region", homeHeader.region)
                            .put("home_battery", homeHeader.battery)
                            .put("home_maker", homeHeader.maker)
                    } else {
                        // GB/GBC/GBA: the board shows a battery only for a
                        // cart that keeps its save on one.
                        game.put("home_battery", homeHeader.battery)
                    }
                    if (!home && !gba) {
                        game.put("gb_title", gbTitle)
                            .put("gb_code", gbCode)
                            .put("gb_japan", gbJapan)
                            .put("gb_cgb_flag", gbCgbFlag)
                    }
                    games.put(game)
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
        val lower = name.lowercase(Locale.ROOT)
        return lower in IGNORED_DIRECTORIES || lower in OTHER_SYSTEM_DIRECTORIES
    }

    private val IGNORED_DIRECTORIES = setOf(
        "bios", "saves", "savestates", "states", "screenshots", "videos",
        "themes", "media", "images", "artwork", "covers", "system volume information",
        // ES-DE's scraped media, gamelists and its own folder: never ROMs.
        "downloaded_media", "gamelists", "manuals", "es-de"
    )

    /**
     * ES-DE ROM folder names of systems slot. does not run (the ES-DE
     * convention most handheld setups follow). Their folders can hold
     * thousands of files, and listing them through SAF is most of a scan's
     * time. Our own systems' folders (gb, gbc, gba, sgb, nes, famicom, snes,
     * sfc, snesna, megadrive, megadrivejp, genesis) and any folder with an
     * unknown name are still searched, so custom layouts keep working.
     */
    private val OTHER_SYSTEM_DIRECTORIES = setOf(
        "3do", "adam", "ags", "amiga", "amiga1200", "amiga600", "amigacd32", "amstradcpc",
        "android", "apple2", "apple2gs", "arcade", "arcadia", "archimedes", "arduboy",
        "astrocde", "atari2600", "atari5200", "atari7800", "atari800", "atarijaguar",
        "atarijaguarcd", "atarilynx", "atarist", "atarixe", "atomiswave", "bbcmicro", "c64",
        "cdimono1", "cdtv", "chailove", "channelf", "coco", "colecovision", "consolearcade",
        "cps", "cps1", "cps2", "cps3", "crvision", "daphne", "desktop", "doom", "dos",
        "dragon32", "dreamcast", "easyrpg", "electron", "emulators", "epic", "fba", "fbneo",
        "fds", "flash", "fm7", "fmtowns", "fpinball", "gamate", "gameandwatch", "gamecom",
        "gamegear", "gc", "gmaster", "gx4000", "intellivision", "j2me", "kodi", "laserdisc",
        "lcdgames", "lowresnx", "lutro", "macintosh", "mame", "mame-advmame", "mastersystem",
        "megacd", "megacdjp", "megaduck", "mess", "model2", "model3", "moto", "msx", "msx1",
        "msx2", "msxturbor", "mugen", "multivision", "n3ds", "n64", "n64dd", "naomi",
        "naomi2", "naomigd", "nds", "neogeo", "neogeocd", "neogeocdjp", "ngage", "ngp",
        "ngpc", "odyssey2", "openbor", "oric", "palm", "pc", "pc88", "pc98", "pcarcade",
        "pcengine", "pcenginecd", "pcfx", "pico8", "plus4", "pokemini", "ports", "ps2", "ps3",
        "ps4", "psp", "psvita", "psx", "pv1000", "quake", "samcoupe", "saturn", "saturnjp",
        "scummvm", "scv", "sega32x", "sega32xjp", "sega32xna", "segacd", "sg-1000",
        "solarus", "spectravideo", "steam", "stv", "supergrafx", "supervision", "supracan",
        "switch", "symbian", "tanodragon", "tg16", "tg-cd", "ti99", "tic80", "to8", "trs-80",
        "uzebox", "vectrex", "vic20", "videopac", "virtualboy", "vpinball", "vsmile",
        "wasm4", "wii", "wiiu", "windows", "windows3x", "windows9x", "wonderswan",
        "wonderswancolor", "x1", "x68000", "xbox", "xbox360", "zmachine", "zx81", "zxnext",
        "zxspectrum"
    )

    private fun cartridgeHeader(context: Context, fileUri: Uri, length: Int): ByteArray {
        return try {
            context.contentResolver.openInputStream(fileUri)?.use { stream ->
                val header = ByteArray(length)
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
