package fyi.slot.konkr

import org.junit.Assert.*
import org.junit.Test

/**
 * Known game identities from the 247-ROM October 2026 Mac AVD inventory.
 * The patched CRCs are deliberately NOT provided to the matcher.
 */
class OfficialArtMatcherTest {
    private data class Case(
        val platform: String, val romName: String,
        val officialName: String, val artCrc: String, val gameCode: String? = null,
    )

    private val examples = listOf(
        Case("GB", "For the Frog the Bell Tolls (Japan) (En) (ryanbgstl) (v1.0)",
            "Kaeru no Tame ni Kane wa Naru (Japan)", "C18CD57A"),
        Case("GB", "Wario Land - Super Mario Land 3 (World) (No Debug) (v1.0) (Searo)",
            "Wario Land - Super Mario Land 3 (World)", "40BE3889"),
        Case("GBA", "Advance Wars (USA) (Rev 1) (No Glitch) (Bartis1989)",
            "Advance Wars (USA) (Rev 1)", "26FD0FC9", "AWRE"),
        Case("GBA", "Apotris (Ultimate Remix Soundpack) (Tudd) (v1.1) (Aftermarket)",
            "Apotris (World) (v4.0.2)", "107F69E0"),
        Case("GBA", "Mother 3 (Japan) (En) (Chewy-Jeffman-Tomato) (v1.3)",
            "Mother 3 (Japan)", "42AC9CB9", "A3UJ"),
        Case("GBA", "Rhythm Tengoku (Japan) (En)",
            "Rhythm Tengoku (Japan)", "349D7025", "BRIJ"),
        Case("GBA", "Super Mario Advance 4 - Super Mario Bros. 3 (USA) (Rev 1) (Switches - RetroAchievements Edition)",
            "Super Mario Advance 4 - Super Mario Bros. 3 (USA, Australia) (Rev 1)",
            "88DAB27F", "AX4E"),
        Case("GBC", "Kirby - Tilt 'n' Tumble (USA) (Accelerometer removal) (infinest) (v1.2)",
            "Kirby - Tilt 'n' Tumble (USA)", "E541ACF1"),
        Case("GBC", "Star Ocean - Blue Sphere (Japan) (En) (v1.0a) (4l3j4ndr0)",
            "Star Ocean - Blue Sphere (Japan) (SGB Enhanced) (GB Compatible)", "8C7DDBDA"),
        Case("GBA", "WarioWare - Twisted! (USA) (Gyro Fix)",
            "WarioWare - Twisted! (USA, Australia)", "CB4E844B", "RZWE"),
    )

    private fun header(code: String? = null): ByteArray = ByteArray(0x150).also { bytes ->
        code?.toByteArray(Charsets.US_ASCII)?.copyInto(bytes, 0xAC)
    }

    private fun catalog(entries: List<Case>, platform: String = entries.first().platform): OfficialArtMatcher.Catalog {
        val text = buildString {
            appendLine("clrmamepro (")
            appendLine(" name \"Nintendo - Test\"")
            appendLine(")")
            for (entry in entries) {
                appendLine("game (")
                appendLine(" name \"${entry.officialName}\"")
                entry.gameCode?.let { appendLine(" serial \"$it\"") }
                appendLine(" rom ( name \"game.rom\" size 8388608 crc ${entry.artCrc} )")
                appendLine(")")
            }
        }
        return OfficialArtMatcher.Catalog.parse(text, platform)
    }

    private fun match(example: Case, entries: List<Case> = listOf(example),
                      available: (String) -> String? = { "GBA/art/$it.png" }): String? =
        OfficialArtMatcher.fromCatalog(
            example.platform, example.romName, header(example.gameCode),
            catalog(entries), available,
        )

    @Test fun allTenTypesOfPatchedGamesResolveWithoutPatchedCrcTables() {
        for (example in examples) assertEquals(
            example.romName, example.artCrc, match(example))
    }

    @Test fun entirelyNewTranslationAndPatchVersionsStillResolve() {
        val mother = examples[4]
        for (name in listOf(
            "Mother 3 (Japan) (En) (Totally New Translation) (v9.9)",
            "Mother 3 (Japan) (En) (Yet Another Patch)",
        )) assertEquals(mother.artCrc, match(mother.copy(romName = name)))

        val wario = examples.last()
        assertEquals(wario.artCrc, match(wario.copy(
            romName = "WarioWare - Twised! (USA) (Unseen Patch)"
        )))
        assertNull(match(wario.copy(
            romName = "WarioWare - Twised! (USA) (Unseen Patch)", gameCode = null
        )))
    }

    @Test fun physicalMother3BeatsVirtualConsoleForEnglishTranslation() {
        val original = examples[4]
        val virtualConsole = original.copy(
            officialName = "Mother 3 (Japan) (Virtual Console)",
            artCrc = "C704A567")
        assertEquals(original.artCrc, match(original, listOf(original, virtualConsole)))
        assertEquals(virtualConsole.artCrc, match(original.copy(
            romName = "Mother 3 (Japan) (Virtual Console)"),
            listOf(original, virtualConsole)))
    }

    @Test fun physicalMarioAdvance4Rev1BeatsSwitchAndVirtualConsole() {
        val physical = examples[6]
        val switch = physical.copy(
            officialName = "Super Mario Advance 4 - Super Mario Bros. 3 (USA) (Rev 1) (Switch Online)",
            artCrc = "22E12D0E")
        val virtualConsole = physical.copy(
            officialName = "Super Mario Advance 4 - Super Mario Bros. 3 (USA) (Rev 1) (Virtual Console)",
            artCrc = "D4C13AC3")
        assertEquals(physical.artCrc,
            match(physical, listOf(switch, virtualConsole, physical)))
    }

    @Test fun apotrisAftermarketSoundpackPrefersNewestAvailableOfficialLabel() {
        val original = examples[3]
        val old = original.copy(officialName = "Apotris (World) (v3.4.5)",
            artCrc = "55AE4312")
        val middle = original.copy(officialName = "Apotris (World) (v4.0.2)",
            artCrc = "107F69E0")
        val newest = original.copy(officialName = "Apotris (World) (v4.1.0)",
            artCrc = "9B777CBA")
        assertEquals(newest.artCrc, match(original, listOf(old, middle, newest)))
        assertEquals(middle.artCrc, match(original.copy(
            romName = "Apotris (World) (v4.0.2)"), listOf(old, middle, newest)))
        // A source that only has two versions remains deterministic.
        assertEquals(middle.artCrc, match(original, listOf(old, middle)))
    }

    @Test fun differentNonVersionedImagesRemainAmbiguous() {
        val base = examples[4]
        val other = base.copy(artCrc = "ABCDEF12")
        assertNull(match(base, listOf(base, other)))
    }

    @Test fun chapterZeroIsNotTheFullGoodboyGalaxyEvercadeGame() {
        val complete = Case("GBA", "Goodboy Galaxy (World)",
            "Goodboy Galaxy (World) (En,Ja,Fr,De,Es,Pt-BR,Zh,Ar) (Evercade)",
            "EBB81069", "XXXX")
        assertNull(match(complete.copy(
            romName = "Goodboy Galaxy - Chapter Zero (World) (Fr) (Aftermarket)",
            gameCode = null), listOf(complete)))
    }

    @Test fun regionCannotSilentlySwitchToOtherMarket() {
        val europe = examples[2].copy(
            officialName = "Advance Wars (Europe) (En,Fr,De,Es)",
            artCrc = "66FB29E9", gameCode = "AWRP")
        assertNull(match(examples[2], listOf(europe)))
    }

    @Test fun internalGbaSerialDisagreementBlocksFalseMatches() {
        val mother = examples[4]
        assertNull(match(mother.copy(gameCode = "XXXX"), listOf(mother)))
    }

    @Test fun ambiguityAcrossDifferentOfficialArtIsRejected() {
        val wario = examples.last()
        val second = wario.copy(artCrc = "ABCDEF12")
        assertNull(match(wario, listOf(wario, second)))
        assertEquals(wario.artCrc, match(wario, listOf(wario, second)) { "one.png" })
    }

    @Test fun exactRevisionWinsAmongOfficialRevisions() {
        val rev = examples[2]
        val base = rev.copy(officialName = "Advance Wars (USA)", artCrc = "DBEF116C")
        assertEquals(rev.artCrc, match(rev, listOf(base, rev)))
    }

    @Test fun unknownOrUnavailableArtIsNotManufactured() {
        val mother = examples[4]
        assertNull(match(mother, available = { null }))
        assertNull(match(mother.copy(romName = "Mother 4 (Japan)")))
        assertNull(OfficialArtMatcher.fromCatalog(
            "SNES", mother.romName, header(), catalog(listOf(mother)),
        ) { "SNES/art/other.png" })
    }

    @Test fun sourceNoIntroMetadataIsParsedCorrectly() {
        val entry = catalog(listOf(examples[2])).entries.single()
        assertEquals("AWRE", entry.serial)
        assertEquals("USA", entry.region)
        assertEquals(1, entry.revision)
        assertEquals("advance wars", entry.titleKey)
        assertEquals("26FD0FC9", entry.crc)
    }
}
