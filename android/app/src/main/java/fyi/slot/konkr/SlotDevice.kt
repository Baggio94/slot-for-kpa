package fyi.slot.konkr

/** How the handheld is named on the About sticker. */
internal object SlotDevice {
    /**
     * Android's model when it already reads as a name ("Retroid Pocket
     * Classic", whose maker is reported as "Moorechip"), else the maker
     * before a bare model code ("RG405M" -> "Anbernic RG405M").
     */
    fun displayName(manufacturer: String?, model: String?): String {
        val maker = manufacturer.orEmpty().trim()
        val name = model.orEmpty().trim()
        if (name.isEmpty()) return maker
        if (maker.isEmpty() || name.contains(' ') || name.startsWith(maker, ignoreCase = true)) return name
        return "$maker $name"
    }
}
