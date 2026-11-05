package fyi.slot.konkr

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.ByteArrayInputStream
import java.util.zip.CRC32

class HomeCartLabelsTest {
    private fun rom(platform: String, name: String, region: String = "", maker: String = "") =
        HomeCartLabels.Rom("content://x", platform, name, "$name.rom", 1, 1, region, maker)

    @Test
    fun shellsFollowTheRegionAndPublisher() {
        assertEquals("fc", HomeCartLabels.shell(rom(Systems.NES, "Super Mario Bros. 3 (Japan)")))
        assertEquals("nes", HomeCartLabels.shell(rom(Systems.NES, "Super Mario Bros. (Japan, USA)")))
        assertEquals("snes_us", HomeCartLabels.shell(rom(Systems.SNES, "Super Mario World (USA)")))
        assertEquals("snes", HomeCartLabels.shell(rom(Systems.SNES, "Super Mario World (Japan)")))
        assertEquals("snes_us", HomeCartLabels.shell(rom(Systems.SNES, "smw", region = "U")))
        assertEquals("md_jp", HomeCartLabels.shell(rom(Systems.MD, "Sonic (Japan)")))
        assertEquals("md_ea", HomeCartLabels.shell(rom(Systems.MD, "Road Rash (USA, Europe)", maker = "(C)T-50 1991.SEP")))
        assertEquals("md", HomeCartLabels.shell(rom(Systems.MD, "Road Rash (Europe)", maker = "(C)T-50 1991.SEP")))
    }

    @Test
    fun labelsComeFromTheShellsRegion() {
        assertEquals(listOf("jp"), HomeCartLabels.labelRegions(rom(Systems.NES, "Dig Dug (Japan)")))
        assertEquals("us", HomeCartLabels.labelRegions(rom(Systems.NES, "Metroid (USA)")).first())
        assertEquals("eu", HomeCartLabels.labelRegions(rom(Systems.NES, "Metroid (Europe)")).first())
        assertEquals(listOf("us"), HomeCartLabels.labelRegions(rom(Systems.SNES, "Super Metroid (USA)")))
        assertEquals("jp", HomeCartLabels.labelRegions(rom(Systems.SNES, "Super Metroid (Japan)")).first())
    }

    @Test
    fun theScanWinsThenTheRenderOfTheRightRegion() {
        val json = JSONObject("""{"response":{"jeu":{"medias":[
            {"type":"support-2D","region":"us","url":"https://a/2d-us"},
            {"type":"support-texture","region":"jp","url":"https://a/tex-jp"},
            {"type":"support-texture","region":"us","url":"https://a/tex-us"},
            {"type":"wheel","region":"us","url":"https://a/wheel"}]}}}""")
        val medias = HomeCartLabels.medias(json)
        assertEquals("https://a/tex-us", HomeCartLabels.pick(medias, rom(Systems.SNES, "Game (USA)"))?.url)
        val noScan = medias.filter { it.type != "support-texture" }
        assertEquals("https://a/2d-us", HomeCartLabels.pick(noScan, rom(Systems.SNES, "Game (USA)"))?.url)
        assertNull(HomeCartLabels.pick(noScan, rom(Systems.SNES, "Game (Europe)")))
    }

    @Test
    fun checksumsSkipDumpHeaders() {
        val body = ByteArray(40000) { (it * 7).toByte() }
        val ines = byteArrayOf(0x4e, 0x45, 0x53, 0x1a) + ByteArray(12) + body
        val (whole, bare) = HomeCartLabels.checksums(ByteArrayInputStream(ines))
        fun crc(b: ByteArray) = CRC32().apply { update(b) }.value.toString(16).uppercase().padStart(8, '0')
        assertEquals(crc(ines), whole)
        assertEquals(crc(body), bare)
        val plain = ByteArray(0x8000) { it.toByte() }
        assertNull(HomeCartLabels.checksums(ByteArrayInputStream(plain)).second)
        val copier = ByteArray(512) + plain
        assertEquals(crc(plain), HomeCartLabels.checksums(ByteArrayInputStream(copier)).second)
    }
}
