package fyi.slot.konkr

import android.app.Activity

/**
 * Android-owned screens borrow Slot's 160 ms menu timing.
 *
 * Native Rust menus already use slot_ui::ease (quintic smoothstep) and must
 * stay native. Android XML uses a close monotone pathInterpolator fit, with
 * the same restrained 8dp vertical travel in both directions.
 *
 * Only apply to app-owned Apps/Cart Studio activities; never override
 * SAF, file pickers, Home settings or other apps' system transitions.
 */
internal object SlotTransitions {
    @Suppress("DEPRECATION")
    fun open(activity: Activity) {
        activity.overridePendingTransition(R.anim.slot_enter, R.anim.slot_hold)
    }

    @Suppress("DEPRECATION")
    fun close(activity: Activity) {
        activity.overridePendingTransition(R.anim.slot_hold, R.anim.slot_exit)
    }
}
