package fyi.slot.konkr

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/**
 * Foreground-only, branded feedback for Slot's Android activities.
 *
 * Android 12's ordinary text Toast supplied a generic Android icon on KONKR,
 * despite the launcher having its own high-resolution adaptive icon. A small
 * custom foreground Toast consistently displays Slot's actual launcher art.
 * No overlay permission, no notification channel, and no background toasts.
 *
 * Exposes the same makeText(context, text, duration).show() interface so all
 * existing app feedback follows the same rendering and wording policy.
 */
internal object SlotToast {
    fun makeText(context: Context, message: CharSequence, duration: Int): Toast =
        makeBrandedToast(context, message, duration, raCompact = false)

    /**
     * RetroAchievements gameplay notices: original Slot palette + typography,
     * but compact and pinned to the upper left instead of the normal footer.
     * All other app toasts retain their existing position and sizing.
     */
    fun makeRaText(context: Context, message: CharSequence, duration: Int): Toast =
        makeBrandedToast(context, message, duration, raCompact = true)

    private fun makeBrandedToast(
        context: Context, message: CharSequence, duration: Int, raCompact: Boolean
    ): Toast {
        val friendly = userMessage(message.toString())
        val density = context.resources.displayMetrics.density
        fun dp(value: Int) = (value * density + 0.5f).toInt()

        return try {
            // Mirror Slot's in-game SAVE STATE / STATE LOADED alerts:
            // flat charcoal plate, quiet hairline, wide-tracked Open Sans.
            val background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                setColor(SlotUi.background)
                setStroke(dp(1), SlotUi.rule)
                cornerRadius = dp(4).toFloat()
            }
            val row = LinearLayout(context).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                if (raCompact) setPadding(dp(11), dp(6), dp(13), dp(6))
                else setPadding(dp(17), dp(9), dp(19), dp(9))
                this.background = background
                elevation = dp(3).toFloat()
            }
            val logo = ImageView(context).apply {
                setImageResource(R.mipmap.ic_launcher)
                contentDescription = "slot."
                scaleType = ImageView.ScaleType.FIT_CENTER
            }
            val logoSize = if (raCompact) 23 else 32
            row.addView(logo, LinearLayout.LayoutParams(dp(logoSize), dp(logoSize)).apply {
                rightMargin = dp(if (raCompact) 9 else 12)
            })
            val text = TextView(context).apply {
                this.text = friendly.uppercase(java.util.Locale.ROOT)
                textSize = 12.5f
                typeface = SlotUi.font(context)
                letterSpacing = .09f
                setTextColor(SlotUi.ink)
                maxLines = 3
                contentDescription = friendly
            }
            row.addView(text, LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
            ))
            @Suppress("DEPRECATION")
            Toast(context.applicationContext).apply {
                this.duration = duration
                if (raCompact) {
                    setGravity(Gravity.TOP or Gravity.START, dp(16), dp(16))
                } else {
                    setGravity(Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL, 0, dp(60))
                }
                view = row
            }
        } catch (_: Exception) {
            // A device-specific toast restriction must never break an action.
            Toast.makeText(context, friendly, duration)
        }
    }

    internal fun userMessage(raw: String): String {
        val msg = raw.trim()
        if (msg.isEmpty()) return "Done."
        return when {
            msg.equals("RAOfflineProxy unavailable", true) ->
                "RAOfflineProxy wasn't found. Install or open it to use achievements."
            msg.equals("RAOfflineProxy stopped", true) ->
                "RAOfflineProxy is stopped. Open it to check achievements."
            msg.equals("RAOfflineProxy address invalid", true) ->
                "RAOfflineProxy is running, but its connection settings need checking."
            msg.contains("ROM folder permission expired", true) ->
                "Games folder access expired. Select it again."
            msg.contains("Choose a ROM folder first", true) ->
                "Choose the folder containing your games first."
            msg.contains("Choose a cartridge first", true) ->
                "Select a game in the carousel first."
            msg.contains("Reopen Slot to show studio changes", true) ->
                "Changes saved. Reopen slot. to refresh your cart labels."
            msg.startsWith("Original Slot. label restored") ->
                "Custom artwork removed. The default label is back."
            msg.startsWith("Slot loaded ") ->
                msg.replaceFirst("Slot loaded ", "Games ready: ")
                    .replace(" carts", " cartridges")
                    .replace(" (limited to 5000)", " (showing the first 5,000)")
            msg.startsWith("Cannot load state:", true) ->
                "Couldn't load the save state. Check your save files."
            msg.startsWith("Cannot export state:", true) ->
                "Save state created, but exporting it failed. Check your save folder."
            msg.startsWith("Cannot open game:", true) ->
                "Can't open this game. Check its ROM and selected core."
            msg.startsWith("Cannot save core selection:", true) ->
                "Couldn't save the selected emulator core."
            msg.startsWith("Cannot save game emulation settings:", true) ->
                "Couldn't save this game's emulation settings."
            msg.startsWith("Multiplayer paused:", true) ->
                "Multiplayer paused. Check the connection before trying again."
            msg.startsWith("Multiplayer:", true) && msg.length > 90 ->
                "Multiplayer connection failed. Check both consoles and retry."
            msg.contains("java.lang.", true) || msg.contains("Exception:", true) ||
                msg.contains("content://", true) ->
                "That didn't work. Check your files and try again."
            else -> msg
        }
    }
}
