package fyi.slot.konkr

import java.text.Normalizer
import java.util.Locale

/**
 * A conservative missing-label fallback, not a replacement for exact CRC matching.
 *
 * Source of truth is the same Libretro No-Intro DAT used by official Cart Studio
 * (see third_party/slot-cart-studio/web/libretro.js). A title is only accepted
 * when it identifies one unambiguous game/art CRC on the SAME platform.
 *
 * Hand-verified patched-ROM equivalences are pinned to both the ROM CRC and
 * its expected title. These are real matches confirmed in Cart Studio, never
 * a fuzzy guess from the game name. A wrong or modified name fails closed.
 */
internal object OfficialArtMatcher {
    internal data class Entry(val name: String, val crc: String, val region: String?)
    internal class Catalog private constructor(
        private val byTitle: Map<String, List<Entry>>,
    ) {
        fun named(title: String): List<Entry> = byTitle[titleKey(title)].orEmpty()

        companion object {
            private val gameStart = Regex("""(?m)^game\s*\(\s*$""")
            private val nameInGame = Regex("""(?m)^\s*name\s+"([^"\r\n]+)"""")
            private val crcInGame = Regex("""\bcrc\s+([0-9a-fA-F]{8})\b""")

            fun parse(dat: String): Catalog {
                // DAT files have 'game (' records with one ROM description on
                // the following lines. Match each record separately: the
                // header's 'name' must never become a candidate.
                val parts = dat.split(gameStart)
                val byName = HashMap<String, MutableList<Entry>>()
                for (block in parts.drop(1)) {
                    val name = nameInGame.find(block)?.groupValues?.get(1) ?: continue
                    val crc = crcInGame.find(block)?.groupValues?.get(1)
                        ?.uppercase(Locale.ROOT) ?: continue
                    byName.getOrPut(titleKey(name), ::ArrayList)
                        .add(Entry(name, crc, region(name)))
                }
                return Catalog(byName)
            }
        }
    }

    private data class Verified(val platform: String, val patchedName: String, val artCrc: String)

    // Pinned against the 247-ROM Mac AVD regression corpus (17 Oct 2026).
    // The left side is the user's patched ROM CRC; the right side is the
    // actual official No-Intro dump chosen manually in Cart Studio.
    // Goodboy Galaxy - Chapter Zero is intentionally NOT mapped to the
    // different full Goodboy Galaxy (Evercade) title.
    private val verified = mapOf(
        "GB:630193E8" to Verified("GB", "For the Frog the Bell Tolls", "C18CD57A"),
        "GB:A728D5E5" to Verified("GB", "Wario Land - Super Mario Land 3", "40BE3889"),
        "GBA:BD8CFEA7" to Verified("GBA", "Advance Wars", "66FB29E9"),
        "GBA:17E5586A" to Verified("GBA", "Apotris", "107F69E0"),
        "GBA:8A3BC5A8" to Verified("GBA", "Mother 3", "42AC9CB9"),
        "GBA:BAF7FFD2" to Verified("GBA", "Rhythm Tengoku", "349D7025"),
        "GBA:CE860480" to Verified("GBA", "Super Mario Advance 4 - Super Mario Bros. 3", "37141F32"),
        "GBC:68B6527F" to Verified("GBC", "Kirby - Tilt 'n' Tumble", "E541ACF1"),
        "GBC:19D2FC31" to Verified("GBC", "Star Ocean - Blue Sphere", "8C7DDBDA"),
        "GBA:9898DD59" to Verified("GBA", "WarioWare - Twisted!", "CB4E844B"),
    )

    private val gameRegion = Regex("""\((USA|Europe|Japan|World)(?:[,)])""", RegexOption.IGNORE_CASE)
    private val nonLetters = Regex("[^a-z0-9]+")
    private val accents = Regex("\\p{M}+")

    private fun baseName(name: String): String = name.substringBefore(" (").trim()
    private fun titleKey(name: String): String {
        val flattened = Normalizer.normalize(baseName(name), Normalizer.Form.NFKD)
            .lowercase(Locale.ROOT).replace(accents, "")
        return flattened.replace(nonLetters, " ").trim()
    }
    private fun region(name: String): String? =
        gameRegion.find(name)?.groupValues?.get(1)?.uppercase(Locale.ROOT)

    /** Overrides are CRC AND title guarded, never filename-only guesses. */
    fun verifiedCrc(platform: String, romCrc: String, title: String): String? {
        val expected = verified["$platform:${romCrc.uppercase(Locale.ROOT)}"] ?: return null
        return expected.artCrc.takeIf {
            expected.platform == platform && titleKey(expected.patchedName) == titleKey(title)
        }
    }

    /** Strict title matching: never silently treat a demo/prologue as a full game. */
    fun fromCatalog(title: String, catalog: Catalog, hasOfficialArt: (String) -> Boolean): String? {
        if (titleKey(title).isBlank()) return null
        val candidates = catalog.named(title).filter { hasOfficialArt(it.crc) }
        if (candidates.isEmpty()) return null
        // If different No-Intro revisions share a title, choose only when
        // there's a single art-bearing candidate for the ROM's region.
        val wishedRegion = region(title)
        val inRegion = if (wishedRegion == null) candidates
            else candidates.filter { it.region == wishedRegion }
        val options = if (inRegion.isNotEmpty()) inRegion else candidates
        val unique = options.map { it.crc }.distinct()
        return unique.singleOrNull()
    }
}
