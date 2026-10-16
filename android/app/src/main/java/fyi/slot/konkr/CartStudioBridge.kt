package fyi.slot.konkr

import android.content.Context
import android.graphics.BitmapFactory
import android.util.AtomicFile
import android.util.Base64
import android.webkit.JavascriptInterface
import org.json.JSONObject
import org.json.JSONArray
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Read-only SAF source for every GB/GBC/GBA ROM already indexed by Slot.
 * The WebView receives opaque keys only; arbitrary URI / filesystem access is
 * never exposed to JavaScript. Label and shell writes remain app-private.
 */
internal class CartStudioBridge(
    private val context: Context,
    private val roms: List<CartStudioCatalog.Rom>,
    private val selectedUri: String?,
    private val dirty: AtomicBoolean,
) {
    private val keyed = roms.associateBy { key(it.uri) }

    // Original Cart Studio writes a label by (platform, stem), not URI.
    // Do not pick the wrong cartridge when two filenames are identical.
    private fun named(platform: String, stem: String): CartStudioCatalog.Rom {
        val matches = roms.filter { it.platform == platform && it.title == stem }
        require(matches.size == 1) { "Ambiguous cartridge name: $platform/$stem" }
        return matches.single()
    }

    private fun known(id: String): CartStudioCatalog.Rom =
        keyed[id] ?: throw IllegalArgumentException("Cartridge is not in Slot library")

    companion object {
        fun key(uri: String): String {
            var hash = -3750763034362895579L
            for (byte in uri.toByteArray(Charsets.UTF_8)) {
                hash = (hash xor (byte.toInt() and 0xff).toLong()) * 0x100000001b3L
            }
            return java.lang.Long.toUnsignedString(hash, 16).padStart(16, '0')
        }

        fun label(context: Context, uri: String): File =
            File(File(context.filesDir, "Labels"), key(uri) + ".png")

        private fun profile(context: Context, uri: String): File =
            File(File(context.filesDir, "Studio"), key(uri) + ".shell")

        fun savedShell(context: Context, uri: String): String? {
            val file = profile(context, uri)
            if (!file.isFile || file.length() !in 1..100) return null
            val raw = file.readText(Charsets.UTF_8).trim()
            return raw.takeIf(::validShell)
        }

        private fun validShell(value: String): Boolean =
            Regex("(auto|notched|rounded) [0-9a-fA-F]{6} (solid|clear|glitter)")
                .matches(value)

        private fun atomicWrite(file: File, bytes: ByteArray) {
            file.parentFile?.mkdirs()
            val atomic = AtomicFile(file)
            val output = atomic.startWrite()
            try {
                output.write(bytes)
                atomic.finishWrite(output)
            } catch (error: Exception) {
                atomic.failWrite(output)
                throw error
            }
        }
    }

    /** Fast initial catalog: no CRC scanning or embedded PNG base64 on load. */
    @JavascriptInterface
    fun session(): String {
        val entries = JSONArray()
        var allReady = true
        for (rom in roms) {
            val labelExists = label(context, rom.uri).isFile
            val cached = CartStudioCatalog.cached(context, rom)
            if (!labelExists && cached == null) allReady = false
            val entry = JSONObject()
                .put("id", key(rom.uri))
                .put("platform", rom.platform)
                .put("stem", rom.title)
                .put("hasLabel", labelExists)
            // A valid size/mtime CRC avoids one Java bridge round-trip per
            // unchanged ROM, and avoids reading its entire contents again.
            if (cached != null) {
                entry.put("fingerprint", JSONObject()
                    .put("crc", cached.crc)
                    .put("head", Base64.encodeToString(cached.head, Base64.NO_WRAP)))
            }
            entries.put(entry)
        }
        return JSONObject()
            .put("carts", entries)
            .put("allCached", allReady)
            .put("selectedKey", selectedUri?.let(::key) ?: "")
            .toString()
    }

    /** Invoked lazily for each cart by the original Studio identify() loop. */
    @JavascriptInterface
    fun fingerprint(id: String): String {
        val fingerprint = CartStudioCatalog.identify(context, known(id))
        require(fingerprint.head.size == 0x150) { "ROM header incomplete" }
        return JSONObject()
            .put("crc", fingerprint.crc)
            .put("head", Base64.encodeToString(fingerprint.head, Base64.NO_WRAP))
            .toString()
    }

    @JavascriptInterface
    fun readLabel(id: String): String {
        // Match saveLabel's maximum; older builds accepted a 9 MiB PNG for
        // Crystal but rejected it on next launch with an opaque Java exception.
        return try {
            val target = label(context, known(id).uri)
            if (!target.isFile) return ""
            require(target.length() in 24..(9L * 1024 * 1024)) {
                "Saved label size unsupported: ${target.length()} bytes"
            }
            Base64.encodeToString(target.readBytes(), Base64.NO_WRAP)
        } catch (error: Exception) {
            android.util.Log.e("SlotStudioRead", "Cannot load saved label: $id", error)
            "error: " + (error.message?.replace("\n", " ")?.take(200)
                ?: error.javaClass.simpleName)
        }
    }

    @Synchronized
    @JavascriptInterface
    fun saveLabel(platform: String, stem: String, encoded: String, replace: Boolean): String {
        // WebView turns a Java exception into the unhelpful generic "Java
        // exception was raised during method invocation". Return a precise
        // result instead, and record the full underlying stack in logcat.
        return try {
            val rom = named(platform, stem)
            // Actual generated PNG labels may exceed 2 MiB, especially GB/GBC
            // full-colour scans. Validate decoded pixels, not a tiny arbitrary
            // compressed size. Never write outside app-private storage.
            require(encoded.length <= 12_000_000) { "Label exceeds 9 MiB" }
            val target = label(context, rom.uri)
            if (!replace && target.isFile) return "skipped"
            val bytes = Base64.decode(encoded, Base64.DEFAULT)
            require(bytes.size in 24..(9 * 1024 * 1024)) { "Invalid PNG length: ${bytes.size}" }
            val signature = byteArrayOf(-119, 80, 78, 71, 13, 10, 26, 10)
            require(bytes.copyOfRange(0, 8).contentEquals(signature)) { "Expected PNG" }
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            require(bounds.outWidth in 1..4096 && bounds.outHeight in 1..4096) {
                "Invalid PNG dimensions ${bounds.outWidth}x${bounds.outHeight}"
            }
            atomicWrite(target, bytes)
            require(target.length() == bytes.size.toLong()) { "Incomplete label write" }
            dirty.set(true)
            "written"
        } catch (error: Exception) {
            android.util.Log.e("SlotStudioSave",
                "Failed to write ${platform}/${stem}: ${error.message}", error)
            val reason = error.message?.replace("\n", " ")?.take(240) ?: error.javaClass.simpleName
            "error: ${reason}"
        }
    }

    @JavascriptInterface
    fun labelShells(): String {
        val lines = roms.mapNotNull { rom ->
            savedShell(context, rom.uri)?.let { rom.title + " = " + it }
        }
        return if (lines.isEmpty()) "" else lines.joinToString("\n", postfix = "\n")
    }

    /**
     * Preserve the upstream merged Labels/cart_shell.txt protocol, mapping
     * each line back to a known SAF ROM. Never change ROMs or system config.
     * A duplicated stem across platforms with different overrides is
     * ambiguous in the original format and is rejected instead of guessing.
     */
    @Synchronized
    @JavascriptInterface
    fun saveShells(text: String): Boolean {
        require(text.length <= 2_000_000) { "Shell data too large" }
        val requested = LinkedHashMap<String, String>()
        for (line in text.lineSequence().map(String::trim).filter(String::isNotEmpty)
            .filterNot { it.startsWith("#") || it.startsWith(";") }) {
            val pair = line.split('=', limit = 2)
            require(pair.size == 2) { "Invalid shell entry" }
            val stem = pair[0].trim()
            require(stem.isNotEmpty() && roms.any { it.title == stem }) { "Unknown cartridge" }
            require(stem !in requested) { "Duplicate cartridge shell entry" }
            val value = pair[1].trim()
            require(value == "auto" || validShell(value)) { "Invalid shell selection" }
            requested[stem] = value
        }
        // Original Studio's key is stem only; matching it across more than
        // one platform is safe only when every occurrence shares the setting.
        for (rom in roms) {
            val desired = requested[rom.title]?.takeUnless { it == "auto" }
            val current = savedShell(context, rom.uri)
            if (desired == current) continue
            val target = profile(context, rom.uri)
            if (desired == null) {
                if (target.exists() && !target.delete()) error("Cannot reset shell")
            } else {
                atomicWrite(target, desired.toByteArray(Charsets.UTF_8))
            }
            dirty.set(true)
        }
        return true
    }
}
