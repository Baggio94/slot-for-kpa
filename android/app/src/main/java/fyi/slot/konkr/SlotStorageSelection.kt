package fyi.slot.konkr

/**
 * User-selected external save paths after applying build-specific test isolation.
 * Keeping this value type in main allows debug and release to select their
 * behavior without adding emulator or lab code to the production APK.
 */
internal data class SlotStorageSelection(
    val savesRoot: String?,
    val statesRoot: String?,
    val shareGbaSaves: Boolean
)
