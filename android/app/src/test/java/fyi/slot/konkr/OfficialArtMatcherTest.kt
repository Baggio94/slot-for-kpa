package fyi.slot.konkr

import org.junit.Assert.*
import org.junit.Test

/** Patch ROMs confirmed by the user's October 2026 Mac AVD CSV and screenshots. */
class OfficialArtMatcherTest {
    private data class Case(
        val platform: String, val patchedCrc: String,
        val rom: String, val officialCrc: String,
    )

    private val confirmed = listOf(
        Case("GB", "630193E8",
            "For the Frog the Bell Tolls (Japan) (En) (ryanbgstl) (v1.0)", "C18CD57A"),
        Case("GB", "A728D5E5",
            "Wario Land - Super Mario Land 3 (World) (No Debug) (v1.0) (Searo)", "40BE3889"),
        Case("GBA", "BD8CFEA7",
            "Advance Wars (USA) (Rev 1) (No Glitch) (Bartis1989)", "66FB29E9"),
        Case("GBA", "17E5586A",
            "Apotris (Ultimate Remix Soundpack) (Tudd) (v1.1) (Aftermarket)", "107F69E0"),
        Case("GBA", "8A3BC5A8",
            "Mother 3 (Japan) (En) (Chewy-Jeffman-Tomato) (v1.3)", "42AC9CB9"),
        Case("GBA", "BAF7FFD2",
            "Rhythm Tengoku (Japan) (En)", "349D7025"),
        Case("GBA", "CE860480",
            "Super Mario Advance 4 - Super Mario Bros. 3 (USA) (Rev 1) (Switches - RetroAchievements Edition)",
            "37141F32"),
        Case("GBC", "68B6527F",
            "Kirby - Tilt 'n' Tumble (USA) (Accelerometer removal) (infinest) (v1.2)", "E541ACF1"),
        Case("GBC", "19D2FC31",
            "Star Ocean - Blue Sphere (Japan) (En) (v1.0a) (4l3j4ndr0)", "8C7DDBDA"),
        Case("GBA", "9898DD59",
            "WarioWare - Twisted! (USA) (Gyro Fix)", "CB4E844B"),
    )

    @Test fun tenManuallyConfirmedPatchedRomsResolveToOfficialArtDumps() {
        for (game in confirmed) {
            assertEquals(game.rom, game.officialCrc,
                OfficialArtMatcher.verifiedCrc(game.platform, game.patchedCrc, game.rom))
        }
    }

    @Test fun aliasRequiresBothMatchingChecksumAndSystemAndGameTitle() {
        for (game in confirmed) {
            assertNull(OfficialArtMatcher.verifiedCrc(
                game.platform, "DEADBEEF", game.rom))
            assertNull(OfficialArtMatcher.verifiedCrc(
                "SNES", game.patchedCrc, game.rom))
            assertNull(OfficialArtMatcher.verifiedCrc(
                game.platform, game.patchedCrc, game.rom.replaceBefore(" (", "Different Title")))
        }
    }

    @Test fun chapterZeroNeverMatchesTheUnrelatedFullEvercadeEdition() {
        val prologue = "Goodboy Galaxy - Chapter Zero (World) (Fr) (v1.0.6) (Aftermarket)"
        assertNull(OfficialArtMatcher.verifiedCrc("GBA", "21512022", prologue))
        val dat = OfficialArtMatcher.Catalog.parse(
            """clrmamepro (
                | name "Nintendo - Game Boy Advance"
                |)
                |game (
                | name "Goodboy Galaxy (World) (En,Ja,Fr,De,Es,Pt-BR,Zh,Ar) (Evercade)"
                | rom ( name "goodboy.gba" size 8388608 crc EBB81069 )
                |)
            """.trimMargin().replace(" |", ""))
        assertNull(OfficialArtMatcher.fromCatalog(prologue, dat) { true })
    }

    private val dat = OfficialArtMatcher.Catalog.parse(
        """clrmamepro (
            |name "Nintendo - Game Boy Advance"
            |)
            |game (
            | name "Advance Wars (Europe) (En,Fr,De,Es)"
            | region "Europe"
            | rom ( name "Advance Wars (Europe).gba" size 4194304 crc 66FB29E9 md5 abc )
            |)
            |game (
            | name "Advance Wars (USA)"
            | region "USA"
            | rom ( name "Advance Wars (USA).gba" size 4194304 crc DBEF116C md5 def )
            |)
            |game (
            | name "Advance Wars (USA) (Rev 1)"
            | region "USA"
            | rom ( name "Advance Wars Rev1.gba" size 4194304 crc 26FD0FC9 md5 ghi )
            |)
            |game (
            | name "WarioWare - Twisted! (USA, Australia)"
            | region "USA"
            | rom ( name "WarioWare Twisted.gba" size 8388608 crc CB4E844B )
            |)
        """.trimMargin()
    )

    @Test fun datParsesOriginalNoIntroClrMameProFormat() {
        assertEquals(3, dat.named("Advance Wars (USA) (No Glitch)").size)
        assertEquals("CB4E844B",
            dat.named("WarioWare - Twisted! (USA, Australia)").single().crc)
    }

    @Test fun onlyAvailableRegionMatchedArtMayBeAppliedAutomatically() {
        assertEquals("DBEF116C", OfficialArtMatcher.fromCatalog(
            "Advance Wars (USA) (hack)", dat) { it == "DBEF116C" })
        assertEquals("66FB29E9", OfficialArtMatcher.fromCatalog(
            "Advance Wars (Europe) (En,Fr,De,Es)", dat) { it == "66FB29E9" })
        assertNull(OfficialArtMatcher.fromCatalog(
            "Advance Wars (Japan)", dat) { it == "DBEF116C" })
    }

    @Test fun multipleEligibleDumpsAreAmbiguousAndMustNotBeSelected() {
        assertNull(OfficialArtMatcher.fromCatalog("Advance Wars (USA) (hack)", dat) {
            it == "DBEF116C" || it == "26FD0FC9"
        })
        assertNull(OfficialArtMatcher.fromCatalog("Advance Wars (USA)", dat) { false })
        assertNull(OfficialArtMatcher.fromCatalog("Advance Wars 2 (USA)", dat) { true })
    }

    @Test fun oneOfficialRomCandidateCanBeReusedForPatchedGame() {
        assertEquals("CB4E844B", OfficialArtMatcher.fromCatalog(
            "WarioWare - Twisted! (USA) (Gyro Fix)", dat) { it == "CB4E844B" })
    }
}
