package fyi.slot.konkr

import java.util.Locale

/**
 * The systems slot. runs, in one place: which cartridge files belong to
 * which shelf, which libretro cores run them (by their libretro key, as
 * the native side names them) and the RetroArch folder each core saves
 * into. Mirrors slot_store::Platform and slot_store::Core.
 */
internal object Systems {
    /** Shelf platform names, as the native side parses them. */
    const val GBA = "GBA"
    const val GB = "GB"
    const val GBC = "GBC"
    const val NES = "NES"
    const val SNES = "SNES"
    const val MD = "MD"

    private val NES_EXT = setOf("nes", "fc", "unf", "unif")
    private val SNES_EXT = setOf("sfc", "smc")
    // .bin is shared with other systems: only a SEGA header makes it a Mega Drive cart.
    private val MD_EXT = setOf("md", "gen", "smd", "bin")

    /** Every cartridge extension, without the .zip container. */
    val ROM_EXTENSIONS: Set<String> = setOf("gba", "gb", "gbc") + NES_EXT + SNES_EXT + MD_EXT

    val HOME_PLATFORMS = setOf(NES, SNES, MD)

    private fun lower(ext: String) = ext.lowercase(Locale.ROOT)

    /** The shelf for a cartridge extension. A .gb may still be a GBC cart. */
    fun platformFor(ext: String): String? = when (lower(ext)) {
        "gba" -> GBA
        "gb" -> GB
        "gbc" -> GBC
        in NES_EXT -> NES
        in SNES_EXT -> SNES
        in MD_EXT -> MD
        else -> null
    }

    fun isHome(ext: String) = platformFor(ext) in HOME_PLATFORMS

    /** libretro key -> RetroArch core name, also its saves/states folder. */
    private val FOLDERS = linkedMapOf(
        "mgba" to "mGBA",
        "gpsp" to "gpSP",
        "gambatte" to "Gambatte",
        "fceumm" to "FCEUmm",
        "nestopia" to "Nestopia",
        "snes9x" to "Snes9x",
        "bsnes" to "bsnes",
        "genesis_plus_gx" to "Genesis Plus GX",
        "picodrive" to "PicoDrive",
    )

    val CORE_FOLDERS: Set<String> = FOLDERS.values.toSet()

    /** The libretro key of a RetroArch core folder: Slot's private file prefix. */
    fun keyForFolder(folder: String): String =
        FOLDERS.entries.firstOrNull { it.value == folder }?.key ?: folder.lowercase(Locale.ROOT)

    /** The two cores of each shelf: the default first. */
    private fun coresFor(platform: String?): List<String> = when (platform) {
        GBA -> listOf("mgba", "gpsp")
        GB, GBC -> listOf("mgba", "gambatte")
        NES -> listOf("fceumm", "nestopia")
        SNES -> listOf("snes9x", "bsnes")
        MD -> listOf("genesis_plus_gx", "picodrive")
        else -> emptyList()
    }

    /** The folder of the core that runs this cart: the one the native side
     *  picked when it runs this extension's system, else the system default. */
    fun coreFolder(key: String, ext: String): String {
        val cores = coresFor(platformFor(ext))
        require(cores.isNotEmpty()) { "Unsupported ROM type" }
        val chosen = key.lowercase(Locale.ROOT).takeIf { it in cores } ?: cores.first()
        return FOLDERS.getValue(chosen)
    }

    /** A save flush names its core by key; unknown keys keep the old mGBA fallback. */
    fun folderForKey(key: String): String = FOLDERS[key.lowercase(Locale.ROOT)] ?: "mGBA"

    /** How many leading bytes the scanner needs to identify a cartridge. */
    fun headerBytes(ext: String): Int = when (platformFor(ext)) {
        // Both LoROM and HiROM headers, behind a possible 512-byte copier header.
        SNES -> 0x10200
        MD -> 0x200
        else -> RomSource.MIN_ROM_BYTES
    }

    /** A .bin only counts as a Mega Drive cart with "SEGA" in its header. */
    fun acceptsHeader(ext: String, header: ByteArray): Boolean {
        if (lower(ext) != "bin") return true
        if (header.size < 0x110) return false
        val text = String(header, 0x100, 0x10, Charsets.ISO_8859_1)
        return text.contains("SEGA")
    }

    /** What the shelf draws from a home console header: the internal
     *  title, the region (J, U, E or empty when unknown) and whether the
     *  cart keeps a battery-backed save. A Mega Drive cart also gives its
     *  copyright field ("(C)T-50 1991.JUN" for Electronic Arts), which
     *  picks the publisher's shell. */
    data class HomeHeader(
        val title: String, val region: String, val battery: Boolean, val maker: String = ""
    )

    /** A Game Boy / Game Boy Color cart keeps its save on a battery when
     *  its cartridge type (header 0x147) says RAM+BATTERY (MBC1/2/3/5/7,
     *  MMM01, HuC1/HuC3, TAMA5, Pocket Camera). */
    fun gbBattery(header: ByteArray): Boolean {
        if (header.size <= 0x147) return false
        return (header[0x147].toInt() and 0xff) in setOf(
            0x03, 0x06, 0x09, 0x0d, 0x0f, 0x10, 0x13, 0x1b, 0x1e, 0x22, 0xfc, 0xfd, 0xfe, 0xff)
    }

    /** A Game Boy Advance cart has a battery when it saves to SRAM, or keeps
     *  a real-time clock (Pokémon Ruby, Sapphire, Emerald...): the save
     *  library and clock driver leave their names in the ROM. Flash and
     *  EEPROM carts need none. Reads the whole cart once, from a stream. */
    fun gbaBattery(input: java.io.InputStream): Boolean {
        val marks = listOf("SRAM_V", "SRAM_F_V", "SIIRTC_V").map { it.toByteArray(Charsets.US_ASCII) }
        val keep = marks.maxOf { it.size } - 1
        val buffer = ByteArray(256 * 1024 + keep)
        var carried = 0
        var total = 0L
        while (true) {
            if (Thread.currentThread().isInterrupted) return false
            val n = input.read(buffer, carried, buffer.size - carried)
            if (n <= 0) break
            total += n
            if (total > 64L * 1024 * 1024) return false
            val end = carried + n
            for (mark in marks) {
                var i = 0
                val last = end - mark.size
                while (i <= last) {
                    if (buffer[i] == mark[0]) {
                        var k = 1
                        while (k < mark.size && buffer[i + k] == mark[k]) k++
                        if (k == mark.size) return true
                    }
                    i++
                }
            }
            carried = minOf(keep, end)
            System.arraycopy(buffer, end - carried, buffer, 0, carried)
        }
        return false
    }

    fun homeHeader(ext: String, header: ByteArray, filename: String): HomeHeader {
        val fromName = regionFromName(filename)
        return when (platformFor(ext)) {
            NES -> nesHeader(header, fromName)
            SNES -> snesHeader(header, fromName)
            MD -> mdHeader(ext, header, fromName)
            else -> HomeHeader("", fromName, false)
        }
    }

    /** No-Intro / GoodTools tags: (Japan), (USA), (Europe), (J), (U), (E)... */
    internal fun regionFromName(filename: String): String {
        val tags = Regex("""\(([^)]*)\)""").findAll(filename)
            .flatMap { it.groupValues[1].split(',').asSequence() }
            .map { it.trim().lowercase(Locale.ROOT) }
            .toList()
        for (tag in tags) {
            when (tag) {
                "japan", "j", "jp", "jpn" -> return "J"
                "usa", "u", "us", "world", "w", "canada" -> return "U"
                "europe", "e", "eu", "pal", "france", "germany", "spain", "italy",
                "uk", "australia", "sweden", "netherlands", "f", "g" -> return "E"
            }
        }
        return ""
    }

    private fun nesHeader(header: ByteArray, region: String): HomeHeader {
        val ines = header.size >= 16 && header[0] == 'N'.code.toByte() &&
            header[1] == 'E'.code.toByte() && header[2] == 'S'.code.toByte() &&
            header[3].toInt() == 0x1a
        // Byte 6 bit 1: battery-backed PRG RAM.
        val battery = ines && (header[6].toInt() and 0x02) != 0
        return HomeHeader("", region, battery)
    }

    private fun snesHeader(header: ByteArray, nameRegion: String): HomeHeader {
        // LoROM then HiROM, each also behind a 512-byte copier header: a
        // valid checksum wins, then a plausible homebrew header.
        val bases = listOf(0x7fc0, 0xffc0, 0x81c0, 0x101c0)
        val base = bases.firstOrNull { checksummed(header, it) }
            ?: bases.firstOrNull { plausible(header, it) }
            ?: return HomeHeader("", nameRegion, false)
        val title = String(header, base, 21, Charsets.ISO_8859_1).trim { it <= ' ' }
        val type = header[base + 0x16].toInt() and 0xff
        // ROM+RAM+battery: low nibble 2, 5 or 6 (also with coprocessors), 9/A (RTC).
        val battery = (type and 0x0f) in setOf(0x02, 0x05, 0x06, 0x09, 0x0a) &&
            (header[base + 0x18].toInt() and 0xff) in 1..0x0c
        val region = when (header[base + 0x19].toInt() and 0xff) {
            0x00 -> "J"
            0x01, 0x0d -> "U"
            in 0x02..0x0c, in 0x10..0x14 -> "E"
            else -> ""
        }
        return HomeHeader(title, nameRegion.ifEmpty { region }, battery)
    }

    private fun checksummed(header: ByteArray, base: Int): Boolean {
        if (base + 0x20 > header.size) return false
        val checksum = (header[base + 0x1e].toInt() and 0xff) or
            ((header[base + 0x1f].toInt() and 0xff) shl 8)
        val complement = (header[base + 0x1c].toInt() and 0xff) or
            ((header[base + 0x1d].toInt() and 0xff) shl 8)
        return checksum xor complement == 0xffff && plausible(header, base)
    }

    // Homebrew often skips the checksum: a printable title and a sane map mode.
    private fun plausible(header: ByteArray, base: Int): Boolean {
        if (base + 0x20 > header.size) return false
        val title = (0 until 21).all { (header[base + it].toInt() and 0xff) in 0x20..0x7e }
        val map = header[base + 0x15].toInt() and 0xff
        return title && (map and 0xe0) == 0x20
    }

    private fun mdHeader(ext: String, header: ByteArray, nameRegion: String): HomeHeader {
        // .smd dumps are interleaved behind a 512-byte header: the region
        // and save fields are not readable as is, so only the name decides.
        if (lower(ext) == "smd" || header.size < 0x200) return HomeHeader("", nameRegion, false)
        val title = String(header, 0x150, 0x30, Charsets.ISO_8859_1).trim { it <= ' ' }
            .ifEmpty { String(header, 0x120, 0x30, Charsets.ISO_8859_1).trim { it <= ' ' } }
        // "RA" then a type byte with bit 6 set: battery-backed SRAM.
        val battery = header[0x1b0] == 'R'.code.toByte() && header[0x1b1] == 'A'.code.toByte() &&
            (header[0x1b2].toInt() and 0x40) != 0
        val codes = String(header, 0x1f0, 3, Charsets.ISO_8859_1).trim().uppercase(Locale.ROOT)
        val region = when {
            'U' in codes -> "U"
            'E' in codes -> "E"
            'J' in codes -> "J"
            // Newer carts write one hex digit: bit 4 USA, bit 8 Europe, bit 1 Japan.
            codes.length == 1 && hexRegion(codes[0], 4) -> "U"
            codes.length == 1 && hexRegion(codes[0], 8) -> "E"
            codes.length == 1 && hexRegion(codes[0], 1) -> "J"
            else -> ""
        }
        val maker = String(header, 0x110, 0x10, Charsets.ISO_8859_1).trim { it <= ' ' }
        return HomeHeader(title, nameRegion.ifEmpty { region }, battery, maker)
    }

    private fun hexRegion(c: Char, bit: Int): Boolean {
        if (c !in '0'..'9' && c !in 'A'..'F') return false
        return (c.digitToInt(16) and bit) != 0
    }
}
