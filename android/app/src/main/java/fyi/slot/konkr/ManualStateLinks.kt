package fyi.slot.konkr

import java.security.MessageDigest
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone

/**
 * Which RetroArch manual slot (.state, .state1 .. .state9) holds which slot.
 * Polaroid, so both sides see the same save states.
 *
 * A link remembers the slot file exactly as it was last synced (hash, size,
 * modification time). A slot that no longer matches its link was written by
 * RetroArch and comes in as a new Polaroid; a Polaroid deleted in slot. only
 * removes its RetroArch copy while that copy still matches its link.
 *
 * Pure logic: no Android storage, covered by JVM tests.
 */
internal object ManualStateLinks {
    /** RetroArch's manual slots 0..9, by file-name suffix after the game stem. */
    val SUFFIXES: List<String> = listOf(".state") + (1..9).map { ".state$it" }

    data class Link(val hash: String, val stamp: String, val modified: Long, val size: Long)

    /** A slot file as listed by its folder, before its bytes are read. */
    data class Seen(val modified: Long, val size: Long)

    fun parse(text: String): Map<String, Link> = text.lineSequence()
        .mapNotNull { line ->
            val f = line.split('\t')
            if (f.size != 5 || f[0] !in SUFFIXES) return@mapNotNull null
            val modified = f[3].toLongOrNull() ?: return@mapNotNull null
            val size = f[4].toLongOrNull() ?: return@mapNotNull null
            f[0] to Link(f[1], f[2], modified, size)
        }
        .toMap()

    fun format(links: Map<String, Link>): String = SUFFIXES
        .mapNotNull { s -> links[s]?.let { "$s\t${it.hash}\t${it.stamp}\t${it.modified}\t${it.size}\n" } }
        .joinToString("")

    /** Slots whose listing alone proves they still hold the synced bytes. */
    fun unchanged(links: Map<String, Link>, seen: Map<String, Seen>): Set<String> =
        seen.filter { (s, file) ->
            links[s]?.let { it.modified == file.modified && it.size == file.size } == true
        }.keys

    /** The slot for a new Polaroid: the first empty one, else the one that
     *  holds the oldest Polaroid and was not changed by RetroArch since.
     *  [same] says whether a slot's current bytes still match its link. */
    fun slotFor(links: Map<String, Link>, present: Set<String>, same: (String) -> Boolean): String? =
        SUFFIXES.firstOrNull { it !in present }
            ?: links.entries.filter { it.key in present }
                .sortedBy { it.value.stamp }
                .firstOrNull { same(it.key) }?.key

    /** Polaroid stamp (UTC, like slot.'s own) for a file modified at [millis],
     *  moved a second later while it clashes with one already [taken]. */
    fun stampFor(millis: Long, taken: Set<String>): String {
        val format = SimpleDateFormat("yyyy-MM-dd_HH-mm-ss", Locale.ROOT)
        format.timeZone = TimeZone.getTimeZone("UTC")
        var seconds = maxOf(0L, millis / 1000)
        var stamp = format.format(Date(seconds * 1000))
        while (stamp in taken) {
            seconds += 1
            stamp = format.format(Date(seconds * 1000))
        }
        return stamp
    }

    fun hash(bytes: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(bytes)
        .joinToString("") { "%02x".format(Locale.ROOT, it.toInt() and 255) }
}
