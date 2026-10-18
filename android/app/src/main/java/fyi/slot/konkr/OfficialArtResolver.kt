package fyi.slot.konkr

import org.json.JSONObject

/** Unified official artwork recognition for Slot and Cart Studio.
 *  Exact ROM CRC wins; patches only resolve through the conservative
 *  No-Intro matcher with matching platform, title, region and GBA game code.
 */
internal object OfficialArtResolver {
    fun artPath(index: JSONObject, platform: String, officialCrc: String): String? =
        index.optJSONObject(officialCrc)?.optString("support-texture")
            ?.takeIf { AutoCartLabels.validArtworkPath(it, platform) }

    fun match(
        rom: CartStudioCatalog.Rom,
        fingerprint: CartStudioCatalog.Fingerprint,
        index: JSONObject,
        dat: OfficialArtMatcher.Catalog?,
    ): String? {
        if (artPath(index, rom.platform, fingerprint.hex) != null) return fingerprint.hex
        if (dat == null) return null
        return OfficialArtMatcher.fromCatalog(
            rom.platform, rom.title, fingerprint.head, dat,
        ) { candidate -> artPath(index, rom.platform, candidate) }
    }
}
