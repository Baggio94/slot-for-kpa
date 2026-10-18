package fyi.slot.konkr

import android.app.Activity
import android.app.Dialog
import android.graphics.drawable.ColorDrawable
import android.widget.FrameLayout
import android.view.ViewGroup
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * RetroAchievements status in Slot's actual OFL label font and flat charcoal
 * panels. The Android password prompt remains native for keyboard/security.
 * Never show account tokens, ROM URIs or contents of the pending ledger.
 */
internal class RaPanelDialog(
    private val activity: Activity,
    private val username: String,
    private val onRefresh: () -> Unit,
    private val onSignOut: () -> Unit,
    private val onDismiss: () -> Unit = {},
) {
    private val ink = SlotUi.ink
    private val muted = SlotUi.muted
    private val border = SlotUi.rule
    private val font: Typeface by lazy { SlotUi.font(activity) }
    private fun dp(x: Int): Int = (x * activity.resources.displayMetrics.density + .5f).toInt()
    private val dialog: Dialog = object : Dialog(
        activity, android.R.style.Theme_Black_NoTitleBar_Fullscreen
    ) {
        override fun dispatchKeyEvent(event: KeyEvent): Boolean {
            val keyCode = event.keyCode
            if (keyCode == KeyEvent.KEYCODE_BUTTON_B) {
                if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount == 0) dismiss()
                return true
            }
            when (keyCode) {
                KeyEvent.KEYCODE_BUTTON_START, KeyEvent.KEYCODE_BUTTON_X -> {
                    if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount == 0) {
                        if (keyCode == KeyEvent.KEYCODE_BUTTON_START) onRefresh()
                        else onSignOut()
                    }
                    return true
                }
                KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN -> {
                    if (event.action == KeyEvent.ACTION_DOWN) {
                        val dy = dp(if (keyCode == KeyEvent.KEYCODE_DPAD_DOWN) 58 else -58)
                        scroll.smoothScrollBy(0, dy)
                    }
                    return true
                }
                KeyEvent.KEYCODE_BUTTON_A -> return true
            }
            return super.dispatchKeyEvent(event)
        }
    }
    private lateinit var content: LinearLayout
    private lateinit var loading: TextView
    private lateinit var actions: LinearLayout
    private lateinit var scroll: ScrollView

    private fun text(message: String, px: Float, color: Int = ink) = TextView(activity).apply {
        this.text = message.uppercase(java.util.Locale.ROOT)
        this.typeface = font
        this.textSize = px
        this.setTextColor(color)
        this.letterSpacing = 0.08f
        includeFontPadding = false
        gravity = Gravity.CENTER_VERTICAL
    }

    private fun separator(): View = View(activity).apply {
        setBackgroundColor(border)
    }

    private fun row(label: String, value: String) {
        val line = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(8), 0, dp(8))
        }
        line.addView(text(label, 13f, muted), LinearLayout.LayoutParams(0, -2, 1f))
        val valueText = text(value, 13f).apply { gravity = Gravity.END or Gravity.CENTER_VERTICAL }
        line.addView(valueText, LinearLayout.LayoutParams(0, -2, 1.55f))
        content.addView(line)
    }

    private fun section(title: String) {
        content.addView(text(title, 14.5f, ink).apply {
            setPadding(0, dp(13), 0, dp(6))
        })
        content.addView(separator(), LinearLayout.LayoutParams(-1, dp(1)))
    }

    private fun badgeProgress(percent: Int?) {
        val pct = percent?.coerceIn(0, 100)
        val rail = FrameLayout(activity).apply {
            setBackgroundColor(Color.rgb(60, 57, 66))
            minimumHeight = dp(7)
        }
        val fill = View(activity).apply {
            setBackgroundColor(SlotUi.ink)
        }
        rail.addView(fill, FrameLayout.LayoutParams(0, dp(7), Gravity.START))
        content.addView(rail, LinearLayout.LayoutParams(-1, dp(7)).apply {
            topMargin = dp(4)
            bottomMargin = dp(8)
        })
        rail.post {
            fill.layoutParams = (fill.layoutParams as FrameLayout.LayoutParams).apply {
                width = if (pct == null) 0 else (rail.width * pct / 100)
            }
        }
    }

    fun show() {
        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(SlotUi.menuPixel(activity, 32),
                SlotUi.menuPixel(activity, 33),
                SlotUi.menuPixel(activity, 32), dp(12))
            setBackgroundColor(SlotUi.menuBackground)
        }
        root.addView(SlotUi.menuHeading(activity, "RETROACHIEVEMENTS"),
            LinearLayout.LayoutParams(-1, SlotUi.menuPixel(activity, 32)))
        root.addView(View(activity), LinearLayout.LayoutParams(-1,
            SlotUi.menuPixel(activity, 18)))
        scroll = ScrollView(activity).apply {
            isFillViewport = false
            isFocusable = false
            isFocusableInTouchMode = false
            isVerticalScrollBarEnabled = false
            clipToPadding = false
        }
        content = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            // Heading's pre-rasterized PNG has a 20px left transparent
            // gutter, scaled by 0.79 = ~16 native pixels. Native menu
            // labels have the SAME raster gutter, TextViews do not.
            // Thus: 16px (visible heading) + 5px (original Slot offset)
            // places actual RA letters at x=53 vs heading letters x=48.
            // Keep the previous right padding for value-column width.
            val visibleHeadingGutter = SlotUi.menuPixel(activity, 16)
            val inset = visibleHeadingGutter + SlotUi.menuEntryInset(activity)
            setPadding(inset, dp(14), SlotUi.menuEntryInset(activity), dp(10))
        }
        // Account information must be visible immediately, even while the
        // first cache diagnostics pass is scanning 115+ ROM definitions.
        section("ACCOUNT")
        row("Player", username)
        row("Connection", "Direct to RA")
        loading = text("CHECKING GAME CACHE AND BADGES...", 12.5f, muted)
        content.addView(loading)
        scroll.addView(content)
        root.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        root.addView(separator(), LinearLayout.LayoutParams(-1, dp(1)))
        actions = LinearLayout(activity).apply {
            gravity = Gravity.CENTER_VERTICAL
            orientation = LinearLayout.HORIZONTAL
            setPadding(0, dp(12), 0, 0)
        }
        // One concise fixed footer. Controller shortcuts do not take D-pad
        // focus; ↑/↓ are reserved exclusively for scrolling statistics.
        for ((key, action, callback) in listOf(
            Triple("START", "REFRESH", onRefresh),
            Triple("X", "SIGN OUT", onSignOut),
            Triple("B", "BACK", { dialog.dismiss() })
        )) {
            val hint = SlotUi.nativeHint(activity, key, action).apply {
                isFocusable = false
                isClickable = true
                setOnClickListener { callback() }
            }
            actions.addView(hint, LinearLayout.LayoutParams(0, dp(39), 1f))
        }
        root.addView(actions)
        dialog.setContentView(root)
        dialog.setOnDismissListener { onDismiss() }
        dialog.show()
        dialog.window?.apply {
            setWindowAnimations(R.style.SlotWindowAnimation)
            setBackgroundDrawable(ColorDrawable(SlotUi.menuBackground))
            addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
            setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
            decorView.systemUiVisibility = (
                View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
                View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
            )
        }
        // No highlighted footer selection; button hints describe direct keys.
    }

    fun update(engine: String, internet: Boolean, snapshot: RaDiagnostics.Snapshot) {
        if (!dialog.isShowing) return
        val previousScroll = scroll.scrollY
        content.removeAllViews()
        section("ACCOUNT")
        row("Player", username)
        row("Engine", if (engine == "Inactive") "Inactive" else "Active")
        // Android's validated Internet is not proof of a successful RA HTTPS
        // handshake. Sync status is derived separately from the native worker.
        row("Connection", if (internet) "Internet available" else "Offline")
        row("Sync", when {
            !internet -> "Waiting for connection"
            engine == "Online" -> "Up to date"
            engine == "Preparing" -> "Preparing"
            engine == "Needs attention" -> "Needs attention"
            engine == "Offline" -> "Waiting for RA service"
            else -> engine
        })
        row("Mode", "Casual")
        section("GAME CACHE")
        row("GBA ROMs", snapshot.libraryGba?.toString() ?: "Not indexed")
        val cached = snapshot.cachedGba?.toString() ?: "Unknown"
        val indexed = snapshot.indexedGba?.toString() ?: "?"
        row("GBA prepared", "$cached / $indexed indexed")
        row("GB prepared", "${snapshot.cachedGb ?: "?"} / ${snapshot.indexedGb ?: "?"} indexed")
        row("GBC prepared", "${snapshot.cachedGbc ?: "?"} / ${snapshot.indexedGbc ?: "?"} indexed")
        row("RA games recognized", snapshot.recognizedGames?.toString() ?: "Checking...")
        row("No RA match", snapshot.unmatchedGames?.toString() ?: "Checking...")
        row("Missing icon IDs", snapshot.missingIconMetadata?.toString() ?: "Checking...")
        row("Game icons cached", if (snapshot.gameIconsTotal != null &&
            snapshot.gameIconsCached != null)
            "${snapshot.gameIconsCached} / ${snapshot.gameIconsTotal}"
            else "Checking cache...")
        section("ACHIEVEMENT BADGES")
        val badgesDownloaded = snapshot.badgesDownloaded
        val badgesTotal = snapshot.badgesTotal
        val imageValue = if (badgesDownloaded != null && badgesTotal != null)
            "$badgesDownloaded / $badgesTotal    ${snapshot.badgesPercent}%"
        else if (badgesTotal != null) "Checking $badgesTotal known badges..."
        else "Not indexed"
        row("Images downloaded", imageValue)
        badgeProgress(snapshot.badgesPercent)
        section("SYNC & AWARDS")
        val budgetUsed = snapshot.prefetchBudgetUsed
        if (budgetUsed != null) {
            row("Cache requests", "$budgetUsed / 100 (30 min)")
            if (snapshot.nextBatchMinutes != null)
                row("Next batch", "In ${snapshot.nextBatchMinutes} min")
        }
        row("Awards to sync", snapshot.pendingAwards?.toString() ?: "Unknown")
        row("Awards saved", snapshot.recordedAwards?.toString() ?: "Unknown")
        row("GB / GBC", "Online + offline (mGBA)")
        // The permanent icon-count explanation was redundant. Show this
        // area only for a genuine local-ledger warning, never for routine
        // offline cache progress. Existing award records are untouched.
        snapshot.warning?.let { warning ->
            content.addView(text(warning, 11f, muted).apply {
                setPadding(0, dp(15), 0, dp(2))
            })
        }
        // The 15-second dashboard refresh must never jump to the top.
        scroll.post { if (dialog.isShowing) scroll.scrollTo(0, previousScroll) }
    }

    fun isShowing(): Boolean = dialog.isShowing

    fun dismiss() = dialog.dismiss()
}
