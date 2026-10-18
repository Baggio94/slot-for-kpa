package fyi.slot.konkr

/**
 * Production KONKR behavior. Mac AVD key remapping and side-by-side
 * multiplayer QA isolation live exclusively in src/debug.
 */
@Suppress("UNUSED_PARAMETER")
internal object SlotBuildBehavior {
    fun verboseEmulationInput(): Boolean = false
    // Android already reports KONKR's controller buttons as native key codes.
    fun emulatorButton(key: Int): Int? = null

    fun storagePolicy(
        packageName: String,
        savesRoot: String?,
        statesRoot: String?,
        shareGbaSaves: Boolean
    ): SlotStorageSelection = SlotStorageSelection(savesRoot, statesRoot, shareGbaSaves)

    fun externalFolderRestrictionMessage(packageName: String): String? = null
}
