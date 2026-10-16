package fyi.slot.konkr

import android.graphics.BitmapFactory
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import java.util.Locale

/**
 * Read-only account diagnostics. Always run off the Android UI thread.
 * Never log ROM URIs, user tokens, or the contents of the pending-award ledger.
 */
internal object RaDiagnostics {
    data class Snapshot(
        val libraryGba: Int?,
        val indexedGba: Int?,
        val cachedGba: Int?,
        val pendingAwards: Int?,
        val recordedAwards: Int?,
        val warning: String?,
        val prefetchBudgetUsed: Int?,
        val nextBatchMinutes: Int?,
        val badgesDownloaded: Int?,
        val badgesTotal: Int?,
        val badgesPercent: Int?,
        val gameIconsCached: Int? = null,
        val gameIconsTotal: Int? = null,
        val recognizedGames: Int? = null,
        val unmatchedGames: Int? = null,
        val missingIconMetadata: Int? = null,
        val libraryGb: Int? = null, val indexedGb: Int? = null, val cachedGb: Int? = null,
        val libraryGbc: Int? = null, val indexedGbc: Int? = null, val cachedGbc: Int? = null,
    )

    private data class PatchEntry(
        val length: Long, val modified: Long,
        val prepared: Boolean, val console: Int, val badgeIds: Set<String>,
        val gameIconName: String?, val recognized: Boolean
    )
    private data class BadgeEntry(val length: Long, val modified: Long, val valid: Boolean)

    // Heavy decoding is performed by the single scanner executor, not Android
    // UI/GL. A change in file size/mtime invalidates a memoized entry.
    private val patches = HashMap<String, PatchEntry>()
    private val badgeFiles = HashMap<String, BadgeEntry>()
    private val lastSnapshots = HashMap<String, Snapshot>()

    @Synchronized
    fun lastSnapshot(username: String): Snapshot? = lastSnapshots[accountKey(username)]

    /** Unknown denominator must not be misrepresented as 0% or 100%. */
    internal fun badgePercentage(downloaded: Int, total: Int): Int? =
        if (total <= 0) null else downloaded.coerceIn(0, total) * 100 / total

    internal fun accountKey(username: String): String {
        val bytes = username.lowercase(Locale.ROOT).toByteArray(Charsets.UTF_8)
        return MessageDigest.getInstance("MD5").digest(bytes)
            .joinToString("") { "%02x".format(it.toInt() and 0xff) }
    }

    private fun readJson(file: File, limit: Long = 8L * 1024 * 1024): JSONObject? =
        if (!file.isFile || file.length() !in 1L..limit) null
        else runCatching { JSONObject(file.readText(Charsets.UTF_8)) }.getOrNull()

    fun inspect(
        root: File, username: String,
        onRulesReady: ((Snapshot) -> Unit)? = null,
    ): Snapshot {
        val account = File(root, "Saves/RetroAchievements/" + accountKey(username))
        val ledger = File(account, "unlocks.json")
        var pending: Int? = 0
        var recorded: Int? = 0
        var warning: String? = null
        if (ledger.exists()) {
            val objectData = readJson(ledger)
            if (objectData == null) {
                pending = null
                recorded = null
                warning = "Local achievements ledger cannot be read. It will be preserved."
            } else {
                val keys = objectData.keys()
                var count = 0
                var unsynced = 0
                var invalid = false
                while (keys.hasNext()) {
                    val award = objectData.optJSONObject(keys.next())
                    if (award == null || !award.has("synced")) { invalid = true; break }
                    count++
                    if (!award.optBoolean("synced")) unsynced++
                }
                if (invalid) {
                    pending = null
                    recorded = null
                    warning = "Local achievements ledger may be damaged. Nothing was deleted."
                } else {
                    pending = unsynced
                    recorded = count
                }
            }
        }

        val shelf = readJson(File(root, "rom-library-cache-v1.json"))
        val index = readJson(File(root, "ra-smart-rom-index-v1.json"))
        val games = shelf?.optJSONArray("games")
        val shelfRoot = shelf?.optString("root")
        val indexed = if (index?.optString("root") == shelfRoot &&
            index?.optInt("schema") == 1
        ) index.optJSONArray("entries") else null

        val hashesByUri = HashMap<String, String>()
        if (indexed != null) {
            for (i in 0 until indexed.length().coerceAtMost(5000)) {
                val item = indexed.optJSONObject(i) ?: continue
                val hash = item.optString("md5")
                if (hash.length == 32 && hash.all { it in '0'..'9' || it in 'a'..'f' }) {
                    hashesByUri[item.optString("uri")] = hash
                }
            }
        }

        val totalByConsole = hashMapOf(4 to 0, 5 to 0, 6 to 0)
        val indexedByConsole = hashMapOf(4 to 0, 5 to 0, 6 to 0)
        val cachedByConsole = hashMapOf(4 to 0, 5 to 0, 6 to 0)
        val discovered = HashMap<String, Int>()
        if (games != null) {
            for (i in 0 until games.length().coerceAtMost(5000)) {
                val item = games.optJSONObject(i) ?: continue
                val console = when (item.optString("platform")) {
                    "GB" -> 4
                    "GBA" -> 5
                    "GBC" -> 6
                    else -> continue
                }
                totalByConsole[console] = totalByConsole.getValue(console) + 1
                val hash = hashesByUri[item.optString("uri")] ?: continue
                if (discovered.putIfAbsent(hash, console) == null) {
                    indexedByConsole[console] = indexedByConsole.getValue(console) + 1
                }
            }
        }
        val budget = readJson(File(account, "smart-prefetch-budget.json"))
        val attempts = budget?.optJSONArray("attempts")
        val now = System.currentTimeMillis() / 1000
        var budgetUsed: Int? = null
        var nextBatchMinutes: Int? = null
        if (attempts != null) {
            val valid = (0 until attempts.length()).map { attempts.optLong(it, -1) }
                .filter { it in (now - 1799)..now }
            budgetUsed = valid.size
            if (valid.size >= 100) {
                val availableAt = (valid.minOrNull() ?: now) + 1800
                nextBatchMinutes = ((availableAt - now + 59) / 60).toInt().coerceAtLeast(1)
            }
        }
        var cached = 0
        // The patch index changes rarely; reuse a parsed patch only if its
        // account-specific file identity and metadata still match.
        val badgeIds = HashSet<String>()
        val gameIcons = HashSet<String>()
        var recognizedGames = 0
        var unmatchedGames = 0
        var missingIconMetadata = 0
        for ((hash, console) in discovered) {
            val file = File(account, "$hash.json")
            if (!file.isFile || file.length() !in 4L..(8L * 1024 * 1024)) continue
            val fileKey = file.absolutePath
            val size = file.length()
            val modified = file.lastModified()
            val old = patches[fileKey]
            val entry = if (old != null && old.length == size && old.modified == modified
                && old.console == console) {
                old
            } else {
                val parsed = runCatching {
                    val data = JSONArray(file.readText(Charsets.UTF_8))
                    val patch = data.optJSONObject(0)
                    if (data.length() != 2 || patch?.optInt("ConsoleID") != console)
                        PatchEntry(size, modified, false, console, emptySet(), null, false)
                    else {
                        val rawIcon = patch.optString("ImageIcon")
                        val iconId = Regex("^/Images/([0-9]{1,12})\\.png$")
                            .matchEntire(rawIcon)?.groupValues?.get(1)
                        val gameIconName = iconId?.let { "slotgame_$it" }
                        val ids = HashSet<String>()
                        val achievements = patch.optJSONArray("Achievements")
                        if (achievements != null) {
                            for (i in 0 until achievements.length()) {
                                val a = achievements.optJSONObject(i) ?: continue
                                if (a.optInt("Flags", 0) != 3) continue
                                val badge = a.optString("BadgeName")
                                if (badge.matches(Regex("[A-Za-z0-9_]{1,64}")))
                                    ids.add(badge)
                            }
                        }
                        PatchEntry(size, modified, true, console, ids.toSet(),
                            gameIconName, patch.optLong("ID", 0) > 0)
                    }
                }.getOrElse {
                    PatchEntry(size, modified, false, console, emptySet(), null, false)
                }
                parsed.also { patches[fileKey] = it }
            }
            if (entry.prepared) {
                cachedByConsole[console] = cachedByConsole.getValue(console) + 1
                cached++
                if (entry.recognized) {
                    recognizedGames++
                    if (entry.gameIconName == null) missingIconMetadata++
                } else unmatchedGames++
                badgeIds.addAll(entry.badgeIds)
                entry.gameIconName?.let(gameIcons::add)
            }
        }

        val preliminary = Snapshot(
            libraryGba = if (games != null) totalByConsole.getValue(5) else null,
            indexedGba = if (games != null) indexedByConsole.getValue(5) else null,
            cachedGba = if (games != null) cachedByConsole.getValue(5) else null,
            libraryGb = if (games != null) totalByConsole.getValue(4) else null,
            indexedGb = if (games != null) indexedByConsole.getValue(4) else null,
            cachedGb = if (games != null) cachedByConsole.getValue(4) else null,
            libraryGbc = if (games != null) totalByConsole.getValue(6) else null,
            indexedGbc = if (games != null) indexedByConsole.getValue(6) else null,
            cachedGbc = if (games != null) cachedByConsole.getValue(6) else null,
            pendingAwards = pending,
            recordedAwards = recorded,
            warning = warning,
            prefetchBudgetUsed = budgetUsed,
            nextBatchMinutes = nextBatchMinutes,
            badgesDownloaded = null,
            badgesTotal = badgeIds.size.takeIf { it > 0 },
            badgesPercent = null,
            gameIconsCached = null,
            gameIconsTotal = gameIcons.size.takeIf { it > 0 },
        )
        onRulesReady?.invoke(preliminary)

        // This is deliberately run on the diagnostics I/O executor, never the
        // UI or GL thread. Bounding file size/dimensions prevents malformed or
        // missing images being counted as successfully downloaded badges.
        val badgeRoot = File(account, "badges")
        var validBadgeCount = 0
        for (name in badgeIds) {
            val file = File(badgeRoot, "$name.png")
            if (!file.isFile || file.length() !in 1L..(128L * 1024)) continue
            val fileKey = file.absolutePath
            val size = file.length()
            val modified = file.lastModified()
            val previous = badgeFiles[fileKey]
            val isValid = if (previous != null &&
                previous.length == size && previous.modified == modified
            ) {
                previous.valid
            } else {
                val decoded = runCatching {
                    val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    BitmapFactory.decodeFile(file.absolutePath, opts)
                    opts.outWidth in 1..256 && opts.outHeight in 1..256
                }.getOrDefault(false)
                badgeFiles[fileKey] = BadgeEntry(size, modified, decoded)
                decoded
            }
            if (isValid) validBadgeCount++
        }
        val validGameIcons = gameIcons.count { name ->
            val file = File(badgeRoot, "$name.png")
            if (!file.isFile || file.length() !in 1L..(128L * 1024)) false
            else {
                val key = file.absolutePath
                val old = badgeFiles[key]
                if (old != null && old.length == file.length() &&
                    old.modified == file.lastModified()) old.valid
                else runCatching {
                    val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    BitmapFactory.decodeFile(key, opts)
                    val good = opts.outWidth in 1..256 && opts.outHeight in 1..256
                    badgeFiles[key] = BadgeEntry(file.length(), file.lastModified(), good)
                    good
                }.getOrDefault(false)
            }
        }
        val badgeTotal = badgeIds.size.takeIf { it > 0 }
        val result = Snapshot(
            libraryGba = if (games != null) totalByConsole.getValue(5) else null,
            indexedGba = if (games != null) indexedByConsole.getValue(5) else null,
            cachedGba = if (games != null) cachedByConsole.getValue(5) else null,
            libraryGb = if (games != null) totalByConsole.getValue(4) else null,
            indexedGb = if (games != null) indexedByConsole.getValue(4) else null,
            cachedGb = if (games != null) cachedByConsole.getValue(4) else null,
            libraryGbc = if (games != null) totalByConsole.getValue(6) else null,
            indexedGbc = if (games != null) indexedByConsole.getValue(6) else null,
            cachedGbc = if (games != null) cachedByConsole.getValue(6) else null,
            pendingAwards = pending,
            recordedAwards = recorded,
            warning = warning,
            prefetchBudgetUsed = budgetUsed,
            nextBatchMinutes = nextBatchMinutes,
            badgesDownloaded = if (badgeTotal != null) validBadgeCount else null,
            badgesTotal = badgeTotal,
            badgesPercent = badgePercentage(validBadgeCount, badgeIds.size),
            gameIconsCached = validGameIcons,
            gameIconsTotal = gameIcons.size,
            recognizedGames = if (games != null) recognizedGames else null,
            unmatchedGames = if (games != null) unmatchedGames else null,
            missingIconMetadata = if (games != null) missingIconMetadata else null,
        )
        synchronized(this) { lastSnapshots[accountKey(username)] = result }
        return result
    }

    /** Only the journal is needed for logout; never scan 115 ROMs + PNGs. */
    fun pendingForSignOut(root: File, username: String): Int? {
        val journal = File(root, "Saves/RetroAchievements/" +
            accountKey(username) + "/unlocks.json")
        if (!journal.exists()) return 0
        val data = readJson(journal) ?: return null
        val keys = data.keys()
        var pending = 0
        while (keys.hasNext()) {
            val award = data.optJSONObject(keys.next()) ?: return null
            if (!award.has("synced")) return null
            if (!award.optBoolean("synced")) pending++
        }
        return pending
    }
}
