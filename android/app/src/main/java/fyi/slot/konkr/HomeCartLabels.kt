package fyi.slot.konkr

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.AtomicFile
import android.util.Base64
import android.util.Log
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import java.util.Locale
import java.util.zip.CRC32

/**
 * Automatic labels for NES / Famicom, Super Nintendo and Mega Drive carts,
 * from ScreenScraper (jeuInfos.php by CRC, then name and size), with slot.'s
 * own developer credentials: the label scan ("support-texture") of the
 * region the shell is drawn in, else the label cut out of the cart's
 * front render ("support-2D"). One request at a time, a batch of carts per pass,
 * nothing during gameplay; a cart ScreenScraper doesn't know is not asked
 * again until its file changes. Manual and Studio labels always win.
 */
internal object HomeCartLabels {
    private const val TAG = "SlotHomeLabels"
    private const val LIBRARY = "rom-library-cache-v1.json"
    private const val STATE = "home-labels-v1.json"
    /** The scanned label kept for Cart Studio's "Real label", by checksum
     *  (the cart's own label file is the one a Studio choice replaces). */
    internal const val REAL_DIR = "home-real-labels"
    private const val REAL_DONE = "home-real-labels/done.json"
    private const val API = "https://api.screenscraper.fr/api2/jeuInfos.php"
    private const val MAX_LIBRARY_BYTES = 8L * 1024 * 1024
    private const val MAX_JSON = 2 * 1024 * 1024
    private const val MAX_IMAGE = 6 * 1024 * 1024
    // One request at a time, but enough carts per pass that a library fills
    // in minutes, not hours (each pass waits for the next shelf idle).
    private const val MAX_SAVED = 24
    /** Labels saved since the app started, for the shelf's counter. */
    @Volatile private var savedTotal = 0
    private const val MAX_ATTEMPTS = 40
    private const val LABEL_MAX = 960

    private val retry = AutoLabelRetryPolicy()
    /** Set when ScreenScraper refuses slot.'s credentials or closes its API. */
    @Volatile private var closedUntil = 0L

    internal data class Rom(
        val uri: String, val platform: String, val title: String, val filename: String,
        val size: Long, val modified: Long, val region: String, val maker: String,
    )

    internal data class Media(val type: String, val region: String, val url: String)

    /** ScreenScraper's system ids. */
    internal fun systemId(platform: String): Int? = when (platform) {
        Systems.NES -> 3
        Systems.SNES -> 4
        Systems.MD -> 1
        else -> null
    }

    private fun credentials(): Triple<String, String, String>? {
        fun read(value: String) = try {
            String(Base64.decode(value, Base64.DEFAULT), Charsets.UTF_8)
        } catch (_: Exception) { "" }
        val id = read(BuildConfig.SCREENSCRAPER_DEV_ID)
        val password = read(BuildConfig.SCREENSCRAPER_DEV_PASSWORD)
        val soft = read(BuildConfig.SCREENSCRAPER_SOFTNAME).ifBlank { "slot" }
        return if (id.isBlank() || password.isBlank()) null else Triple(id, password, soft)
    }

    // ---------------------------------------------------------- regions ---

    /** The regions a No-Intro / GoodTools name lists: (Japan, USA, Europe). */
    internal fun nameRegions(filename: String): Triple<Boolean, Boolean, Boolean>? {
        var j = false; var u = false; var e = false
        Regex("""[(\[]([^)\]]*)[)\]]""").findAll(filename).forEach { m ->
            m.groupValues[1].split(',').map { it.trim().lowercase(Locale.ROOT) }.forEach {
                when (it) {
                    "japan", "j", "jp", "jpn", "asia" -> j = true
                    "usa", "u", "us", "canada", "world", "w" -> u = true
                    "europe", "e", "eu", "pal", "france", "germany", "spain", "italy", "uk",
                    "australia", "sweden", "netherlands", "f", "g", "scandinavia", "brazil" -> e = true
                }
            }
        }
        return if (j || u || e) Triple(j, u, e) else null
    }

    /** The shell slot. draws for this cart (slot-ui's home_shell, without the
     *  Famicom table): its label must come from the same region. */
    internal fun shell(rom: Rom): String {
        val regions = nameRegions(rom.filename.ifBlank { rom.title })
        val japanOnly = regions?.let { it.first && !it.second && !it.third } ?: (rom.region == "J")
        val usa = regions?.second ?: (rom.region == "U")
        val maker = rom.maker.uppercase(Locale.ROOT)
        return when (rom.platform) {
            Systems.NES -> if (japanOnly) "fc" else "nes"
            Systems.SNES -> if (usa) "snes_us" else "snes"
            Systems.MD -> when {
                japanOnly -> "md_jp"
                usa && ("T-50" in maker || "ELECTRONIC ARTS" in maker) -> "md_ea"
                else -> "md"
            }
            else -> ""
        }
    }

    /** ScreenScraper media regions whose labels fit the shell, best first. */
    internal fun labelRegions(rom: Rom): List<String> {
        val europe = listOf("eu", "fr", "de", "uk", "sp", "it", "nl", "se", "au")
        val regions = nameRegions(rom.filename.ifBlank { rom.title })
        return when (shell(rom)) {
            "fc", "md_jp" -> listOf("jp")
            "snes_us", "md_ea" -> listOf("us")
            // Super Famicom labels have the PAL shape.
            "snes" -> if (regions?.first == true && regions.third != true)
                listOf("jp") + europe + listOf("wor") else europe + listOf("jp", "wor")
            else -> if (regions?.second == true && regions.third != true)
                listOf("us", "wor") + europe + listOf("ss") else europe + listOf("us", "wor", "ss")
        }
    }

    /** The label well inside a cart's front render, as fractions of its
     *  outline (slot-ui's mould table): x, y, w, h. */
    private val WINDOWS = mapOf(
        "nes" to floatArrayOf(126f / 312, 0f / 352, 145f / 312, 239f / 352),
        "snes" to floatArrayOf(22f / 312, 11f / 215, 269f / 312, 92f / 215),
        "snes_us" to floatArrayOf(63f / 312, 0f / 200, 185f / 312, 84f / 200),
        "md" to floatArrayOf(48f / 312, 0f / 210, 216f / 312, 179f / 210),
        "md_jp" to floatArrayOf(39f / 312, 0f / 209, 232f / 312, 175f / 209),
        "md_ea" to floatArrayOf(62f / 312, 0f / 290, 218f / 312, 240f / 290),
    )

    /** The label to fetch: the region's scan, else its front render when
     *  slot. knows where that shell's label sits. */
    internal fun pick(medias: List<Media>, rom: Rom): Media? {
        val regions = labelRegions(rom)
        fun best(type: String) = regions.firstNotNullOfOrNull { region ->
            medias.firstOrNull { it.type == type && it.region == region }
        }
        return best("support-texture")
            ?: best("support-2D")?.takeIf { shell(rom) in WINDOWS }
    }

    internal fun medias(json: JSONObject): List<Media> {
        val list = json.optJSONObject("response")?.optJSONObject("jeu")
            ?.optJSONArray("medias") ?: return emptyList()
        return (0 until list.length()).mapNotNull { i ->
            val m = list.optJSONObject(i) ?: return@mapNotNull null
            val url = m.optString("url")
            if (!url.startsWith("https://")) return@mapNotNull null
            Media(m.optString("type"), m.optString("region"), url)
        }
    }

    // ------------------------------------------------------------ games ---

    private fun games(context: Context): List<Rom> {
        val file = File(context.filesDir, LIBRARY)
        if (!file.isFile || file.length() !in 1..MAX_LIBRARY_BYTES) return emptyList()
        val root = try { JSONObject(file.readText(Charsets.UTF_8)) } catch (_: Exception) { return emptyList() }
        val rows = root.optJSONArray("games") ?: return emptyList()
        return (0 until rows.length().coerceAtMost(5000)).mapNotNull { i ->
            val row = rows.optJSONObject(i) ?: return@mapNotNull null
            val platform = row.optString("platform")
            val uri = row.optString("uri")
            if (platform !in Systems.HOME_PLATFORMS || !uri.startsWith("content://")) return@mapNotNull null
            Rom(uri, platform, row.optString("title"), row.optString("filename"),
                row.optLong("size", -1), row.optLong("modified", -1),
                row.optString("home_region"), row.optString("home_maker"))
        }
    }

    /** CRC32 of the whole file and without a dump header (iNES's 16 bytes,
     *  a copier's 512), streamed once. */
    internal fun checksums(input: InputStream): Pair<String, String?> {
        val whole = CRC32()
        val after16 = CRC32()
        val after512 = CRC32()
        val head = ByteArray(16)
        var total = 0L
        val buffer = ByteArray(32768)
        input.use { stream ->
            while (true) {
                val n = stream.read(buffer)
                if (n < 0) break
                if (n == 0) continue
                whole.update(buffer, 0, n)
                if (total < 16) System.arraycopy(buffer, 0, head, total.toInt(), minOf(n.toLong(), 16 - total).toInt())
                fun skip(crc: CRC32, from: Long) {
                    val start = (from - total).coerceIn(0, n.toLong()).toInt()
                    if (start < n) crc.update(buffer, start, n - start)
                }
                skip(after16, 16)
                skip(after512, 512)
                total += n
            }
        }
        fun hex(c: CRC32) = java.lang.Long.toHexString(c.value).uppercase(Locale.ROOT).padStart(8, '0')
        val ines = head[0] == 'N'.code.toByte() && head[1] == 'E'.code.toByte() &&
            head[2] == 'S'.code.toByte() && head[3].toInt() == 0x1a
        val bare = when {
            ines -> hex(after16)
            total % 0x2000 == 512L -> hex(after512)
            else -> null
        }
        return hex(whole) to bare
    }

    // ------------------------------------------------------------- state ---

    private fun state(context: Context): JSONObject =
        try { JSONObject(File(context.filesDir, STATE).readText(Charsets.UTF_8)) }
        catch (_: Exception) { JSONObject() }

    private fun saveState(context: Context, state: JSONObject) {
        val atomic = AtomicFile(File(context.filesDir, STATE))
        val out = atomic.startWrite()
        try {
            out.write(state.toString().toByteArray(Charsets.UTF_8))
            atomic.finishWrite(out)
        } catch (error: Exception) {
            atomic.failWrite(out)
            throw error
        }
    }

    private fun realDone(context: Context): JSONObject =
        try { JSONObject(File(context.filesDir, REAL_DONE).readText(Charsets.UTF_8)) }
        catch (_: Exception) { JSONObject() }

    private fun writeAtomic(file: File, bytes: ByteArray) {
        file.parentFile?.mkdirs()
        val atomic = AtomicFile(file)
        val out = atomic.startWrite()
        try {
            out.write(bytes)
            atomic.finishWrite(out)
        } catch (error: Exception) {
            atomic.failWrite(out)
            throw error
        }
    }

    private fun known(state: JSONObject, rom: Rom): Boolean {
        val old = state.optJSONObject(rom.uri) ?: return false
        return old.optLong("size") == rom.size && old.optLong("modified") == rom.modified
    }

    // ------------------------------------------------------------- fetch ---

    private class Http(val code: Int, val body: ByteArray)

    private fun get(url: String, limit: Int, shouldContinue: () -> Boolean): Http {
        val connection = (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 9_000
            readTimeout = 12_000
            instanceFollowRedirects = true
            setRequestProperty("User-Agent", "slot. for Android")
        }
        try {
            val code = connection.responseCode
            val stream = if (code in 200..299) connection.inputStream else connection.errorStream
            val out = ByteArrayOutputStream()
            stream?.use { input ->
                val buffer = ByteArray(16 * 1024)
                while (shouldContinue()) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    if (out.size() + n > limit) throw IllegalStateException("response too large")
                    out.write(buffer, 0, n)
                }
            }
            return Http(code, out.toByteArray())
        } finally {
            connection.disconnect()
        }
    }

    private fun lookup(rom: Rom, crc: String, creds: Triple<String, String, String>,
                       shouldContinue: () -> Boolean): Http {
        fun enc(value: String) = URLEncoder.encode(value, "UTF-8")
        val (id, password, soft) = creds
        val url = "$API?devid=${enc(id)}&devpassword=${enc(password)}&softname=${enc(soft)}" +
            "&output=json&romtype=rom&systemeid=${systemId(rom.platform)}&crc=$crc" +
            "&romnom=${enc(rom.filename.ifBlank { rom.title })}" +
            (if (rom.size > 0) "&romtaille=${rom.size}" else "")
        return get(url, MAX_JSON, shouldContinue)
    }

    /** The label as stored for the shelf: the scan as it is (slot. covers it
     *  onto the label well), or the well cut out of the front render. */
    internal fun labelPng(bytes: ByteArray, media: Media, rom: Rom): ByteArray? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth !in 32..4096 || bounds.outHeight !in 32..4096) return null
        val source = BitmapFactory.decodeByteArray(bytes, 0, bytes.size) ?: return null
        var cut: Bitmap? = null
        var scaled: Bitmap? = null
        try {
            var image = source
            if (media.type == "support-2D") {
                val window = WINDOWS[shell(rom)] ?: return null
                // The render's outline: its opaque bounding box.
                var x0 = image.width; var y0 = image.height; var x1 = -1; var y1 = -1
                val row = IntArray(image.width)
                for (y in 0 until image.height) {
                    image.getPixels(row, 0, image.width, 0, y, image.width, 1)
                    for (x in row.indices) if ((row[x] ushr 24) > 128) {
                        if (x < x0) x0 = x; if (x > x1) x1 = x
                        if (y < y0) y0 = y; if (y > y1) y1 = y
                    }
                }
                if (x1 <= x0 || y1 <= y0) return null
                val w = x1 - x0 + 1; val h = y1 - y0 + 1
                val lx = x0 + (window[0] * w).toInt(); val ly = y0 + (window[1] * h).toInt()
                val lw = (window[2] * w).toInt().coerceAtMost(image.width - lx)
                val lh = (window[3] * h).toInt().coerceAtMost(image.height - ly)
                if (lw < 16 || lh < 16) return null
                cut = Bitmap.createBitmap(image, lx, ly, lw, lh)
                image = cut
            }
            val k = minOf(1f, LABEL_MAX.toFloat() / maxOf(image.width, image.height))
            if (k < 1f) {
                scaled = Bitmap.createScaledBitmap(image, (image.width * k).toInt().coerceAtLeast(1),
                    (image.height * k).toInt().coerceAtLeast(1), true)
                image = scaled
            }
            val out = ByteArrayOutputStream()
            return if (image.compress(Bitmap.CompressFormat.PNG, 100, out)) out.toByteArray() else null
        } finally {
            scaled?.recycle()
            cut?.recycle()
            source.recycle()
        }
    }

    /** Games Cart Studio asked for that ScreenScraper doesn't know, this run. */
    private val studioMisses = java.util.Collections.synchronizedSet(HashSet<String>())

    /** The label of a home game picked by hand in Cart Studio (not one of the
     *  library's carts, so not scraped yet): fetched from ScreenScraper when
     *  Studio shows it and kept with the others, under each of its checksums
     *  tried in turn (a dump's own, then its headerless one). */
    fun studioLabel(context: Context, platform: String, name: String, crcs: List<String>): ByteArray? {
        if (systemId(platform) == null || crcs.isEmpty()) return null
        val dir = File(context.filesDir, REAL_DIR)
        crcs.map { File(dir, "$it.png") }.firstOrNull { it.isFile && it.length() in 8L..MAX_IMAGE.toLong() }
            ?.let { return it.readBytes() }
        val key = platform + ":" + crcs.joinToString(",")
        if (key in studioMisses) return null
        val creds = credentials() ?: return null
        if (android.os.SystemClock.elapsedRealtime() < closedUntil) return null
        val rom = Rom("", platform, name, name, -1, -1, "", "")
        val go = { true }
        for (crc in crcs) {
            val reply = lookup(rom, crc, creds, go)
            when (reply.code) {
                200 -> {
                    val media = pick(medias(JSONObject(String(reply.body, Charsets.UTF_8))), rom)
                        ?: break
                    val image = get(media.url, MAX_IMAGE, go)
                    if (image.code != 200) return null
                    val png = labelPng(image.body, media, rom) ?: break
                    writeAtomic(File(dir, "$crc.png"), png)
                    return png
                }
                404 -> Thread.sleep(150L)
                400, 401, 403, 423, 426 -> {
                    closedUntil = android.os.SystemClock.elapsedRealtime() + 3_600_000L
                    Log.w(TAG, "ScreenScraper refused the request: HTTP ${reply.code}")
                    return null
                }
                else -> return null
            }
        }
        studioMisses.add(key)
        return null
    }

    /** Returns true while carts are left for a later pass. */
    fun fill(context: Context, shouldContinue: () -> Boolean, onSaved: (String) -> Unit): Boolean {
        val creds = credentials() ?: return false
        val now = android.os.SystemClock.elapsedRealtime()
        if (now < closedUntil || !shouldContinue()) return false
        val state = state(context)
        val real = realDone(context)
        val all = games(context)
        // A cart is missing its label, or (scraped before Studio offered it)
        // the copy Cart Studio shows as its real label.
        val missing = all.filter {
            (!CartLabelPaths.file(context.filesDir, it.uri).exists() || !real.has(it.uri)) &&
                !known(state, it) && retry.pending(it.uri)
        }
        if (missing.isEmpty()) return false
        // The shelf's "LABELS n/total" counter, for the home carts' own pass.
        var checked = all.size - missing.size
        AutoCartLabelDiagnostics.start(checked, all.size, savedTotal, AutoCartLabelDiagnostics.HOME)
        var saved = 0
        var attempts = 0
        var dirty = false
        for (rom in missing.filter { retry.ready(it.uri, now) }) {
            if (!shouldContinue() || saved >= MAX_SAVED || attempts >= MAX_ATTEMPTS) break
            attempts++
            var outcome = "NOT_FOUND"
            try {
                val stream = context.contentResolver.openInputStream(Uri.parse(rom.uri))
                    ?: throw IllegalStateException("ROM unreadable")
                val (whole, bare) = RomSource.useRom(stream, rom.filename.ifBlank { rom.title }) { input, _ ->
                    checksums(input)
                }
                var reply: Http? = null
                for (crc in listOfNotNull(whole, bare).distinct()) {
                    if (!shouldContinue()) break
                    reply = lookup(rom, crc, creds, shouldContinue)
                    if (reply.code != 404) break
                    Thread.sleep(150L)
                }
                if (!shouldContinue()) { outcome = "INTERRUPTED"; break }
                val answer = reply ?: continue
                when (answer.code) {
                    200 -> {}
                    404 -> { outcome = "NOT_FOUND"; continue }
                    400, 401, 403, 423, 426 -> {
                        // Refused credentials or a closed API: stop for an hour.
                        closedUntil = android.os.SystemClock.elapsedRealtime() + 3_600_000L
                        Log.w(TAG, "ScreenScraper refused the request: HTTP ${answer.code}")
                        outcome = "INTERRUPTED"
                        break
                    }
                    else -> { outcome = "NETWORK_ERROR"; continue }
                }
                val json = JSONObject(String(answer.body, Charsets.UTF_8))
                val media = pick(medias(json), rom) ?: continue
                Thread.sleep(100L)
                val image = get(media.url, MAX_IMAGE, shouldContinue)
                if (image.code != 200) { outcome = "NETWORK_ERROR"; continue }
                val png = labelPng(image.body, media, rom) ?: continue
                for (crc in listOfNotNull(whole, bare).distinct()) writeAtomic(File(File(context.filesDir, REAL_DIR), "$crc.png"), png)
                real.put(rom.uri, true)
                dirty = true
                val target = CartLabelPaths.file(context.filesDir, rom.uri)
                if (target.exists()) { outcome = "EXISTS"; saved++; continue }
                target.parentFile?.mkdirs()
                val atomic = AtomicFile(target)
                val out = atomic.startWrite()
                try {
                    out.write(png)
                    atomic.finishWrite(out)
                } catch (error: Exception) {
                    atomic.failWrite(out)
                    throw error
                }
                saved++
                savedTotal++
                outcome = "SAVED"
                onSaved(rom.uri)
            } catch (error: Exception) {
                outcome = "NETWORK_ERROR"
                Log.w(TAG, "Label deferred for ${rom.title.take(80)}", error)
            } finally {
                when (outcome) {
                    "SAVED", "EXISTS" -> retry.finished(rom.uri)
                    "NOT_FOUND" -> {
                        // ScreenScraper has no label for this file: don't ask
                        // again until the file changes.
                        retry.finished(rom.uri)
                        state.put(rom.uri, JSONObject().put("size", rom.size)
                            .put("modified", rom.modified))
                        dirty = true
                    }
                    "NETWORK_ERROR" -> retry.transient(rom.uri, android.os.SystemClock.elapsedRealtime())
                }
                if (!retry.pending(rom.uri)) checked++
                AutoCartLabelDiagnostics.result(checked, savedTotal, outcome,
                    AutoCartLabelDiagnostics.HOME)
                Log.i(TAG, "HOME_ART_RESULT $outcome [${rom.platform}] ${rom.title.take(96)}")
            }
            Thread.sleep(250L)
        }
        if (dirty) try {
            saveState(context, state)
            writeAtomic(File(context.filesDir, REAL_DONE), real.toString().toByteArray(Charsets.UTF_8))
        } catch (error: Exception) {
            Log.w(TAG, "Could not remember label misses", error)
        }
        val left = missing.any { retry.pending(it.uri) }
        return left && shouldContinue() && android.os.SystemClock.elapsedRealtime() >= closedUntil
    }
}
