package fyi.slot.konkr

import java.io.File

/**
 * The same unsigned FNV-1a path identity as slot-android's core_selection::key.
 * Opaque SAF URIs must never be replaced with filename-only label keys.
 */
internal object CartLabelPaths {
    fun file(filesDir: File, romUri: String): File {
        var hash = -3750763034362895579L // 0xcbf29ce484222325
        for (byte in romUri.toByteArray(Charsets.UTF_8)) {
            hash = (hash xor (byte.toInt() and 0xff).toLong()) * 0x100000001b3L
        }
        val name = java.lang.Long.toUnsignedString(hash, 16).padStart(16, '0')
        return File(File(filesDir, "Labels"), "$name.png")
    }
}
