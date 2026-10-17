package fyi.slot.konkr

import java.text.Normalizer
import java.util.Locale
import kotlin.math.max

/**
 * General No-Intro artwork resolver. It NEVER depends on patched-ROM CRCs:
 * CRC already had its chance in AutoCartLabels. This fallback uses the same
 * platform-specific No-Intro DAT as original Cart Studio, with the game name,
 * region, revision and (on GBA) the internal 4-byte cartridge Game Code.
 *
 * Fuzzy title matching is allowed ONLY when a matching GBA Game Code is
 * present. Other consoles need an exact canonical title/verified translation
 * title; low-confidence matches are not automatically painted.
 */
internal object OfficialArtMatcher {
    internal data class Entry(
        val name: String,
        val crc: String,
        val region: String?,
        val revision: Int?,
        val serial: String?,
        val titleKey: String,
    )

    internal class Catalog private constructor(
        val platform: String, val entries: List<Entry>,
    ) {
        companion object {
            private val gameStart = Regex("""(?m)^game\s*\(\s*$""")
            private val nameLine = Regex("""(?m)^\s*name\s+"([^"\r\n]+)"""")
            private val crcLine = Regex("""\bcrc\s+([0-9a-fA-F]{8})\b""")
            private val serialLine = Regex("""\bserial\s+"([A-Za-z0-9]{4})"""")

            fun parse(dat: String, platform: String): Catalog {
                require(platform in setOf("GB", "GBC", "GBA")) { "Unsupported platform" }
                require(dat.startsWith("clrmamepro (")) { "Not a No-Intro DAT" }
                val entries = ArrayList<Entry>()
                for (block in dat.split(gameStart).drop(1)) {
                    val name = nameLine.find(block)?.groupValues?.get(1) ?: continue
                    val crc = crcLine.find(block)?.groupValues?.get(1)
                        ?.uppercase(Locale.ROOT) ?: continue
                    entries.add(Entry(
                        name, crc, region(name), revision(name),
                        serialLine.find(block)?.groupValues?.get(1)?.uppercase(Locale.ROOT),
                        titleKey(name),
                    ))
                }
                require(entries.isNotEmpty()) { "No games in No-Intro DAT" }
                return Catalog(platform, entries)
            }
        }
    }

    private val regionTag = Regex("""\((USA|Europe|Japan|World)(?:[,)])""",
        RegexOption.IGNORE_CASE)
    private val revisionTag = Regex("""\(Rev\s+(\d+)\)""", RegexOption.IGNORE_CASE)
    private val sourceVersionTag = Regex("""\(v(\d+(?:\.\d+){1,3})\)""", RegexOption.IGNORE_CASE)
    private val mark = Regex("\\p{M}+")
    private val separator = Regex("[^a-z0-9]+")
    private val alternateTitles = mapOf(
        // International title of the Japanese Game Boy release. This maps a
        // TITLE, not a particular patched dump's CRC and works for any patch.
        "for the frog the bell tolls" to "kaeru no tame ni kane wa naru",
    )

    private fun region(name: String): String? =
        regionTag.find(name)?.groupValues?.get(1)?.uppercase(Locale.ROOT)

    private fun revision(name: String): Int? =
        revisionTag.find(name)?.groupValues?.get(1)?.toIntOrNull()

    internal fun titleKey(name: String): String {
        val base = name.substringBefore(" (").trim()
        val flat = Normalizer.normalize(base, Normalizer.Form.NFKD)
            .lowercase(Locale.ROOT).replace(mark, "")
        return flat.replace(separator, " ").trim()
    }

    private fun gameCode(platform: String, head: ByteArray): String? {
        // The GBA game code at 0xAC..0xAF survives most IPS/BPS translation
        // patches and is also recorded in No-Intro as the 4-char "serial".
        if (platform != "GBA" || head.size < 0xB0) return null
        val code = (0xAC..0xAF).map { head[it].toInt() and 0xff }
        if (!code.all { it in 48..57 || it in 65..90 }) return null
        return code.map(Int::toChar).joinToString("")
    }

    // A storefront reissue is not the physical cartridge. Preserve an
    // explicitly requested edition, but never let an unlabelled translation
    // accidentally resolve to a Switch Online or Virtual Console image.
    private fun digitalEdition(name: String): String? = when {
        name.contains("(Virtual Console)", true) -> "VC"
        name.contains("(Switch Online)", true) -> "SWITCH"
        else -> null
    }

    private fun sourceVersion(name: String): List<Int>? =
        sourceVersionTag.find(name)?.groupValues?.get(1)?.split('.')
            ?.mapNotNull { it.toIntOrNull() }
            ?.takeIf { it.size in 2..4 }

    private fun compareSourceVersions(left: List<Int>, right: List<Int>): Int {
        for (i in 0 until max(left.size, right.size)) {
            val difference = left.getOrElse(i) { 0 }.compareTo(right.getOrElse(i) { 0 })
            if (difference != 0) return difference
        }
        return 0
    }

    private fun similarity(a: String, b: String): Double {
        if (a.isEmpty() || b.isEmpty()) return 0.0
        if (a == b) return 1.0
        // Bounded Levenshtein. The DAT titles are short; no large or
        // attacker-controlled allocations while the app is foregrounded.
        if (a.length > 120 || b.length > 120) return 0.0
        var previous = IntArray(b.length + 1) { it }
        var current = IntArray(b.length + 1)
        for (i in a.indices) {
            current[0] = i + 1
            for (j in b.indices) {
                current[j + 1] = minOf(
                    current[j] + 1,
                    previous[j + 1] + 1,
                    previous[j] + if (a[i] == b[j]) 0 else 1,
                )
            }
            val temp = previous
            previous = current
            current = temp
        }
        return 1.0 - previous[b.length].toDouble() / max(a.length, b.length)
    }

    /**
     * @return a real official No-Intro CRC, only when the best evidence is
     * unique. Identical artwork PATHS on different official revisions count as
     * one result. An ambiguous or wrong-region game returns null.
     */
    fun fromCatalog(
        platform: String,
        romTitle: String,
        header: ByteArray,
        catalog: Catalog,
        artworkPath: (String) -> String?,
    ): String? {
        if (platform != catalog.platform) return null
        val requested = titleKey(romTitle)
        if (requested.isBlank()) return null
        val wanted = alternateTitles[requested] ?: requested
        val expectedRegion = region(romTitle)
        val expectedRevision = revision(romTitle)
        val requestedEdition = digitalEdition(romTitle)
        val requestedVersion = sourceVersion(romTitle)
        val code = gameCode(platform, header)

        data class Candidate(val entry: Entry, val path: String, val score: Int)
        val candidates = ArrayList<Candidate>()
        for (entry in catalog.entries) {
            // DAT is loaded separately for each system by OfficialDatSource.
            // Never cross system boundaries or silently choose foreign-region
            // art for a ROM with a known region.
            if (expectedRegion != null && entry.region != expectedRegion) continue
            val edition = digitalEdition(entry.name)
            if (requestedEdition != null && edition != requestedEdition) continue

            val serial = entry.serial
            val exactCode = code != null && serial == code
            val familyCode = code != null && serial != null &&
                serial.take(3) == code.take(3)

            // A recognizable but conflicting GBA serial is a strong signal
            // the filename does NOT describe the game in the ROM.
            if (code != null && serial != null && !familyCode) continue

            val exactTitle = entry.titleKey == wanted
            val fuzzy = !exactTitle && exactCode &&
                wanted.split(' ').size >= 2 &&
                similarity(wanted, entry.titleKey) >= 0.88
            if (!exactTitle && !fuzzy) continue

            val path = artworkPath(entry.crc) ?: continue
            var score = if (exactTitle) 100 else 60
            if (exactCode) score += 25
            else if (familyCode) score += 10
            if (expectedRevision != null) {
                if (entry.revision == expectedRevision) score += 12
                else score -= 12
            } else if (entry.revision == null) {
                score += 5
            }
            if (requestedEdition == null && edition != null) score -= 35
            // On aftermarket software the ROM's patch revision may differ
            // from the official game's version. Prefer an exact underlying
            // version when known, otherwise resolve version-only variants
            // conservatively after collecting all eligible candidates.
            if (requestedVersion != null &&
                requestedVersion == sourceVersion(entry.name)) score += 25
            // No title match may override a serial conflict; and fuzzy
            // matches without an exact full code never qualify.
            candidates.add(Candidate(entry, path, score))
        }
        val best = candidates.maxOfOrNull { it.score } ?: return null
        val winners = candidates.filter { it.score == best }
        if (winners.map { it.path }.distinct().size == 1) {
            return winners.first().entry.crc
        }
        // All eligible entries describe the *same game* and differ only
        // by official aftermarket version (e.g. Apotris v3/v4). When a
        // patched soundpack has its own unrelated version tag, prefer the
        // newest verified official label, never a different game or edition.
        // A mixed physical/VC/Switch, region or Rev set stays ambiguous.
        if (winners.any { digitalEdition(it.entry.name) != null } ||
            winners.map { it.entry.revision }.distinct().size != 1 ||
            winners.map { it.entry.region }.distinct().size != 1 ||
            winners.map { it.entry.titleKey }.distinct().size != 1) return null
        val versioned = winners.map { it to sourceVersion(it.entry.name) }
        if (versioned.any { it.second == null }) return null
        val newest = versioned.maxWithOrNull { a, b ->
            compareSourceVersions(a.second!!, b.second!!)
        } ?: return null
        val latest = versioned.filter {
            compareSourceVersions(it.second!!, newest.second!!) == 0
        }
        if (latest.map { it.first.path }.distinct().size != 1) return null
        return latest.first().first.entry.crc
    }
}
