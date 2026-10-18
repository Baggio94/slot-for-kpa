package fyi.slot.konkr

import android.content.Context
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout

/**
 * One transparent, self-reflowing rail under the shelf Wi-Fi / Bluetooth
 * indicators. Both tasks share a parent: GONE removes a completed task from
 * layout, so the remaining task immediately occupies the first position.
 *
 * Only the Android UI thread changes the views. The scan workers publish
 * immutable progress snapshots and never touch this layout.
 */
internal class SlotShelfTaskIndicators(context: Context, frame: FrameLayout) {
    private val scale =
        (context.resources.displayMetrics.widthPixels / 720f).coerceIn(1f, 2f)
    private fun dim(value: Int) = (value * scale + .5f).toInt()
    private val rail = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.END
        isClickable = false
        isFocusable = false
        clipChildren = false
        clipToPadding = false
        visibility = View.VISIBLE
        // Transparent: no opaque status panel on either Slot theme.
    }

    val library = SlotLibraryRefreshIndicator(context, rail)
    val labels = SlotAutoLabelIndicator(context, rail)

    init {
        frame.addView(rail, FrameLayout.LayoutParams(-2, -2,
            Gravity.TOP or Gravity.END).apply {
            // Radio row: y=12..36; task rail starts below it.
            topMargin = dim(40)
            rightMargin = dim(17)
        })
    }

    fun setLightTheme(light: Boolean) {
        library.setLightTheme(light)
        labels.setLightTheme(light)
    }
}
