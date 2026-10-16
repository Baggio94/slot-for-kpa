package fyi.slot.konkr

import android.content.Context
import android.net.Uri
import android.util.Log
import java.io.File
import java.security.MessageDigest

/**
 * Best-effort cartridge ROM warming on an independent, low-priority executor.
 *
 * The real launcher retains sole ownership of core setup, RetroArch imports,
 * save conflict arbitration and all emulation state. This cache ONLY primes
 * the exact private file the original launch path already consumes.
 */
internal object RomWarmCache {
    private const val TAG = "SlotRomWarm"
    private const val MAX_ROM = 64L * 1024 * 1024
    private const val MAX_TOTAL = 256L * 1024 * 1024

    fun prepare(context: Context, rawUri: String, shouldContinue: () -> Boolean) {
        if (!rawUri.startsWith("content://") || !shouldContinue()) return
        val uri = Uri.parse(rawUri)
        val ext = uri.lastPathSegment.orEmpty().lowercase()
            .substringAfterLast('.', "")
        if (ext !in setOf("gba", "gbc", "gb")) return
        val digest = MessageDigest.getInstance("SHA-256")
            .digest(rawUri.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it.toInt() and 0xff) }
        val dir = File(context.cacheDir, "rom-cache").apply { mkdirs() }
        val target = File(dir, "$digest.$ext")
        if (target.isFile && target.length() >= 0x150L) return

        val temporary = File(dir, "$digest.warm.tmp")
        try {
            if (!shouldContinue()) return
            val source = context.contentResolver.openInputStream(uri) ?: return
            var count = 0L
            source.use { input ->
                temporary.outputStream().buffered().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        if (!shouldContinue() || Thread.currentThread().isInterrupted) return
                        val n = input.read(buffer)
                        if (n < 0) break
                        count += n
                        if (count > MAX_ROM) return
                        output.write(buffer, 0, n)
                        // Yield gently to core, audio, UI and cartridge artwork.
                        if ((count and 0x7ffffL) == 0L) Thread.sleep(1L)
                    }
                }
            }
            if (count < 0x150 || !shouldContinue()) return
            // Do not replace a file materialized concurrently by the real
            // launcher; all writes are atomic from the perspective of readers.
            if (!target.exists()) {
                if (!temporary.renameTo(target)) return
                Log.i(TAG, "Warmed selected ROM ($count bytes)")
            }
            // Cache-only files. No saves, screenshots, labels or states touched.
            trim(dir, target)
        } catch (error: Exception) {
            Log.w(TAG, "Optional ROM warming skipped", error)
        } finally {
            temporary.delete()
        }
    }

    private fun trim(dir: File, selected: File) {
        val files = dir.listFiles()?.filter {
            it.isFile && it.name.matches(Regex("[a-f0-9]{64}\\.(gba|gbc|gb)"))
        }?.sortedByDescending { it.lastModified() } ?: return
        var kept = 0L
        for (file in files) {
            if (file == selected) { kept += file.length(); continue }
            if (kept + file.length() > MAX_TOTAL) file.delete()
            else kept += file.length()
        }
    }
}
