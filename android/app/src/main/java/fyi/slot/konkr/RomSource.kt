package fyi.slot.konkr

import java.io.BufferedInputStream
import java.io.FilterInputStream
import java.io.InputStream
import java.util.Locale
import java.util.zip.ZipInputStream

/**
 * One ROM per cartridge source: a plain .gba/.gbc/.gb file, or the FIRST
 * matching cartridge within a .zip. Non-ROM extras (artwork, README) are
 * ignored. Archives with multiple games should be separated by the user:
 * one cartridge is displayed per ZIP, never a synthetic multi-ROM shelf.
 *
 * Streams only from the SAF provider. The original source is never modified.
 * Bounded uncompressed bytes prevent ZIP bombs and excessive cache storage.
 */
internal object RomSource {
    const val MAX_ROM_BYTES: Long = 64L * 1024 * 1024
    const val MIN_ROM_BYTES = 0x150
    private const val MAX_ENTRIES = 256
    private val supported = setOf("gba", "gbc", "gb")

    data class Header(val extension: String, val bytes: ByteArray)

    fun isContainer(filename: String): Boolean =
        filename.substringAfterLast('.', "").lowercase(Locale.ROOT) == "zip"

    fun isAcceptedFile(filename: String): Boolean =
        filename.substringAfterLast('.', "").lowercase(Locale.ROOT) in supported + "zip"

    private fun extension(filename: String): String =
        filename.substringAfterLast('.', "").lowercase(Locale.ROOT)

    /**
     * The ZIP entry is exposed only while the callback is running; callers
     * must consume it synchronously and never keep a reference to the stream.
     * No entry path is written to the filesystem (zip-slip is impossible).
     */
    fun <T> useRom(source: InputStream, filename: String,
                   operation: (InputStream, String) -> T): T {
        val ext = extension(filename)
        require(ext in supported + "zip") { "Unsupported cartridge type" }
        if (ext != "zip") {
            return source.use { operation(Limited(it), ext) }
        }
        return ZipInputStream(BufferedInputStream(source)).use { zip ->
            var visited = 0
            while (visited++ < MAX_ENTRIES) {
                val entry = zip.nextEntry ?: break
                if (entry.isDirectory) continue
                val romExt = extension(entry.name.substringAfterLast('/'))
                if (romExt !in supported) continue
                require(entry.size == -1L || entry.size in MIN_ROM_BYTES.toLong()..MAX_ROM_BYTES) {
                    "ZIP ROM exceeds safe size limits"
                }
                // Preserve a deterministic first-ROM choice across scan,
                // warming, artwork/RA fingerprints and emulator launch.
                return@use operation(Limited(zip), romExt)
            }
            throw IllegalArgumentException("ZIP does not contain a GB, GBC or GBA ROM")
        }
    }

    fun inspect(source: InputStream, filename: String): Header =
        useRom(source, filename) { stream, ext ->
            val head = ByteArray(MIN_ROM_BYTES)
            var filled = 0
            while (filled < head.size) {
                val n = stream.read(head, filled, head.size - filled)
                if (n < 0) break
                if (n == 0) continue
                filled += n
            }
            require(filled == MIN_ROM_BYTES) { "Cartridge ROM header is too small" }
            Header(ext, head)
        }

    private class Limited(source: InputStream): FilterInputStream(source) {
        private var received = 0L
        override fun read(): Int {
            val byte = super.read()
            if (byte >= 0) {
                received++
                require(received <= MAX_ROM_BYTES) { "ROM exceeds 64 MiB uncompressed" }
            }
            return byte
        }
        override fun read(b: ByteArray, off: Int, len: Int): Int {
            val n = `in`.read(b, off, len)
            if (n > 0) {
                received += n
                require(received <= MAX_ROM_BYTES) { "ROM exceeds 64 MiB uncompressed" }
            }
            return n
        }
        // The owner, not this limited view, closes the SAF/ZIP stream.
        override fun close() {}
    }
}
