package fyi.slot.konkr

import android.os.Build
import android.view.KeyEvent

/**
 * Development-only behavior for the Android ARM64 emulator on Mac and the
 * side-by-side multiplayer QA install. Not packaged in the release APK.
 */
internal object SlotBuildBehavior {
    private val isAndroidEmulator by lazy {
        val fingerprint = Build.FINGERPRINT.lowercase()
        val hardware = Build.HARDWARE.lowercase()
        fingerprint.startsWith("generic") || fingerprint.contains("sdk_gphone") ||
            hardware.contains("ranchu") || hardware.contains("goldfish")
    }

    fun emulatorButton(key: Int): Int? {
        if (!isAndroidEmulator) return null
        return when (key) {
            KeyEvent.KEYCODE_Z -> KeyEvent.KEYCODE_BUTTON_A
            KeyEvent.KEYCODE_X -> KeyEvent.KEYCODE_BUTTON_B
            KeyEvent.KEYCODE_A -> KeyEvent.KEYCODE_BUTTON_X
            KeyEvent.KEYCODE_S -> KeyEvent.KEYCODE_BUTTON_Y
            KeyEvent.KEYCODE_Q -> KeyEvent.KEYCODE_BUTTON_L1
            KeyEvent.KEYCODE_E -> KeyEvent.KEYCODE_BUTTON_R1
            KeyEvent.KEYCODE_1 -> KeyEvent.KEYCODE_BUTTON_L2
            KeyEvent.KEYCODE_2 -> KeyEvent.KEYCODE_BUTTON_R2
            KeyEvent.KEYCODE_SPACE -> KeyEvent.KEYCODE_BUTTON_SELECT
            KeyEvent.KEYCODE_ENTER -> KeyEvent.KEYCODE_BUTTON_START
            KeyEvent.KEYCODE_TAB -> KeyEvent.KEYCODE_BUTTON_MODE
            else -> null // Arrow keys are already Android D-pad events.
        }
    }

    private fun isolatedLab(packageName: String): Boolean =
        packageName == "fyi.slot.konkr.lab"

    fun storagePolicy(
        packageName: String,
        savesRoot: String?,
        statesRoot: String?,
        shareGbaSaves: Boolean
    ): SlotStorageSelection =
        if (isolatedLab(packageName)) SlotStorageSelection(null, null, false)
        else SlotStorageSelection(savesRoot, statesRoot, shareGbaSaves)

    fun externalFolderRestrictionMessage(packageName: String): String? =
        if (isolatedLab(packageName))
            "Multiplayer Lab: external save folders disabled to protect Slot saves."
        else null
}
