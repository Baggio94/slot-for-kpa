package fyi.slot.konkr

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import java.io.File
import java.util.ArrayDeque
import java.util.Locale
import java.util.concurrent.Executors

/**
 * RetroArch-inspired achievement card with Slot's REAL upstream label.ttf.
 * An in-Activity overlay, NOT an Android Toast: precise upper-left placement,
 * cached achievement badge art, no SYSTEM_ALERT_WINDOW permission and no
 * changes to the EGL surface, input, emulation or award synchronisation.
 */
internal class RaAchievementOverlay(
    private val activity: Activity,
    parent: FrameLayout,
) {
    data class Notice(
        val title: String,
        val detail: String,
        val earned: Boolean,
        val badge: String?,
        // Nullable means ordinary RA notice (not a game-launch status).
        // For the launch notice, true/false is the Android-validated internet
        // snapshot, independent of whether RA data came from the local cache.
        val onlineMode: Boolean? = null,
    )

    private val handler = Handler(Looper.getMainLooper())
    private val loader = Executors.newSingleThreadExecutor { task ->
        Thread(task, "slot-ra-badge-ui").apply { priority = Thread.MIN_PRIORITY }
    }
    private val queue = ArrayDeque<Notice>()
    private var showing = false
    private var currentNotice: Notice? = null
    private var destroyed = false
    private var serial = 0
    private val ink = SlotUi.ink
    private val secondary = SlotUi.muted
    private val accent = SlotUi.accent
    private val face: Typeface by lazy { SlotUi.font(activity) }
    private fun dp(x: Int) = (activity.resources.displayMetrics.density * x + 0.5f).toInt()

    // Android 12 clips a one-pixel GradientDrawable stroke at some display
    // densities. Draw this inset outline in the foreground AFTER text/images:
    // all four edges remain visible, including the right-hand side.
    private val outline = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(126, 122, 132)
        style = Paint.Style.STROKE
        strokeWidth = dp(2).toFloat()
    }
    private val card = object : LinearLayout(activity) {
        override fun onDrawForeground(canvas: Canvas) {
            super.onDrawForeground(canvas)
            val inset = outline.strokeWidth / 2 + dp(1)
            if (width > dp(4) && height > dp(4)) {
                canvas.drawRoundRect(
                    RectF(inset, inset, width - inset, height - inset),
                    dp(4).toFloat(), dp(4).toFloat(), outline
                )
            }
        }
    }.apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(12), dp(9), dp(14), dp(9))
        background = GradientDrawable().apply {
            // 92% opaque: subtly reveal gameplay under the popup, without
            // fading the achievement badge, text or crisp foreground border.
            setColor(Color.argb(235, 23, 22, 25))
            cornerRadius = dp(4).toFloat()
        }
        elevation = dp(3).toFloat()
        isClickable = false
        isFocusable = false
        visibility = View.GONE
    }
    private val badge = ImageView(activity).apply {
        scaleType = ImageView.ScaleType.FIT_CENTER
        contentDescription = "RetroAchievements badge"
    }
    private val badgeFallback = TextView(activity).apply {
        text = "★"
        // Keep the approved star, but render its backing label consistently.
        typeface = face
        letterSpacing = .09f
        includeFontPadding = false
        textSize = 24f
        gravity = Gravity.CENTER
        setTextColor(accent)
        background = GradientDrawable().apply {
            setColor(Color.rgb(47, 43, 40))
            cornerRadius = dp(3).toFloat()
        }
    }
    private val texts = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
    private val heading = label(10f, accent)
    private val title = label(14.2f, ink).apply {
        setSingleLine(true)
        ellipsize = android.text.TextUtils.TruncateAt.END
    }
    private val modeLabel = label(9.5f, accent).apply {
        setSingleLine(true)
        visibility = View.GONE
        letterSpacing = .05f
    }
    private val gameLine = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
    }
    private val subtitle = label(10.5f, secondary).apply { maxLines = 2 }

    init {
        card.addView(badge, LinearLayout.LayoutParams(dp(46), dp(46)).apply {
            rightMargin = dp(11)
        })
        card.addView(badgeFallback, LinearLayout.LayoutParams(dp(46), dp(46)).apply {
            rightMargin = dp(11)
        })
        texts.addView(heading)
        // WRAP_CONTENT keeps the mode directly after SHORT titles even when
        // a long subtitle makes the notification card wider.
        gameLine.addView(title, LinearLayout.LayoutParams(-2, -2))
        gameLine.addView(modeLabel, LinearLayout.LayoutParams(-2, -2).apply {
            leftMargin = dp(8)
        })
        texts.addView(gameLine)
        texts.addView(subtitle)
        card.addView(texts, LinearLayout.LayoutParams(0, -2, 1f))
        parent.addView(card, FrameLayout.LayoutParams(dp(358), -2, Gravity.TOP or Gravity.START).apply {
            leftMargin = dp(15)
            topMargin = dp(14)
        })
        card.bringToFront()
    }

    private fun label(size: Float, colour: Int) = TextView(activity).apply {
        typeface = face
        textSize = size
        setTextColor(colour)
        letterSpacing = 0.09f
        includeFontPadding = false
        gravity = Gravity.CENTER_VERTICAL
        ellipsize = android.text.TextUtils.TruncateAt.END
    }

    /** Only called on Activity's main thread. Never blocks gameplay. */
    fun push(notice: Notice) {
        if (destroyed) return
        // Keep real unlocked rewards over ordinary status updates if busy.
        if (queue.size >= 12) {
            val iterator = queue.iterator()
            var removedStatus = false
            while (iterator.hasNext()) {
                if (!iterator.next().earned) {
                    iterator.remove()
                    removedStatus = true
                    break
                }
            }
            if (!removedStatus) queue.removeFirst()
        }
        queue.addLast(notice)
        if (!showing) showNext()
    }

    private fun showNext() {
        if (destroyed) return
        val notice = queue.pollFirst() ?: run {
            showing = false
            currentNotice = null
            card.visibility = View.GONE
            return
        }
        showing = true
        currentNotice = notice
        val seq = ++serial
        val status = notice.title.trim()
        if (notice.earned) {
            heading.text = "ACHIEVEMENT EARNED  •  SYNC PENDING"
            title.text = status.uppercase(Locale.ROOT)
            modeLabel.visibility = View.GONE
            subtitle.text = notice.detail.uppercase(Locale.ROOT)
            subtitle.visibility = if (notice.detail.isBlank()) View.GONE else View.VISIBLE
            badge.visibility = View.GONE
            badgeFallback.text = "★"
            badgeFallback.visibility = View.VISIBLE
            loadBadge(notice.badge, seq)
        } else {
            heading.text = "RETROACHIEVEMENTS"
            title.text = status.uppercase(Locale.ROOT)
            // Two independent single-line labels guarantee that the mode
            // can never wrap below a long game title.
            modeLabel.visibility = if (notice.onlineMode != null) View.VISIBLE else View.GONE
            modeLabel.text = when (notice.onlineMode) {
                true -> "ONLINE MODE"
                false -> "OFFLINE MODE"
                null -> ""
            }
            modeLabel.setTextColor(if (notice.onlineMode == true) accent else secondary)
            subtitle.text = notice.detail.uppercase(Locale.ROOT)
            subtitle.visibility = if (notice.detail.isBlank()) View.GONE else View.VISIBLE
            badge.visibility = View.GONE
            badgeFallback.text = "RA"
            badgeFallback.visibility = if (notice.badge != null) View.VISIBLE else View.GONE
            loadBadge(notice.badge, seq)
        }
        // Prefer the natural text width and enlarge only as far as the screen
        // allows. Exceptionally long titles ellipsize rather than wrapping or
        // shrinking the status mode out of view.
        val maxWidth = (activity.resources.displayMetrics.widthPixels * .85f).toInt()
        val minWidth = minOf(dp(315), maxWidth)
        val textPixels = maxOf(
            title.paint.measureText(title.text.toString()) +
                if (modeLabel.visibility == View.VISIBLE)
                    modeLabel.paint.measureText(modeLabel.text.toString()) + dp(8)
                else 0f,
            subtitle.paint.measureText(subtitle.text.toString()),
            heading.paint.measureText(heading.text.toString())
        )
        val desired = (textPixels + dp(46 + 11 + 12 + 14)).toInt()
        (card.layoutParams as? FrameLayout.LayoutParams)?.let {
            it.width = desired.coerceIn(minWidth, maxWidth)
            card.layoutParams = it
            val otherContent = dp(46 + 11 + 12 + 14)
            val modeWidth = if (modeLabel.visibility == View.VISIBLE)
                modeLabel.paint.measureText(modeLabel.text.toString()).toInt() + dp(8)
                else 0
            // Long titles ellipsize while the ON/OFFLINE MODE label remains
            // fully visible on its own single line.
            title.maxWidth = (it.width - otherContent - modeWidth).coerceAtLeast(dp(95))
        }
        card.visibility = View.VISIBLE
        card.animate().cancel()
        // Never translate the card beyond its 15dp inset: Android would clip
        // the left border during the entrance. Fade the full card in place.
        card.translationX = 0f
        card.alpha = 0f
        card.animate().alpha(1f).setDuration(220).start()
        handler.postDelayed({
            if (seq == serial && !destroyed) {
                card.animate().alpha(0f)
                    .setDuration(180).withEndAction {
                        if (seq == serial && !destroyed) showNext()
                    }.start()
            }
        }, if (notice.earned) 3600L else if (notice.badge != null) 3200L else 1750L)
    }

    /** Called after game ejection, never for a real earned-achievement toast.
     * A stale launch notification must not remain on the empty cartridge shelf.
     */
    fun dismissGameLaunchNotices() {
        if (destroyed) return
        queue.removeAll { it.onlineMode != null }
        if (currentNotice?.onlineMode == null) return
        // Invalidate delayed fades and pending asynchronous icon loads.
        val cancelled = ++serial
        currentNotice = null
        card.animate().cancel()
        card.animate().alpha(0f).setDuration(130L).withEndAction {
            if (!destroyed && serial == cancelled) {
                card.visibility = View.GONE
                showNext() // earned notices, if any, are preserved
            }
        }.start()
    }

    private fun loadBadge(path: String?, seq: Int) {
        if (path.isNullOrBlank()) return
        loader.execute {
            // The native HTTP worker may still be downloading this exact badge.
            // Retry for a short bounded window, never downloading on UI/GL.
            for (attempt in 0 until 7) {
                if (Thread.currentThread().isInterrupted || destroyed || serial != seq) break
                val bitmap = loadValidatedBadge(path)
                if (bitmap != null) {
                    handler.post {
                        if (seq == serial && !destroyed) {
                            badge.setImageBitmap(bitmap)
                            badge.visibility = View.VISIBLE
                            badgeFallback.visibility = View.GONE
                        } else bitmap.recycle()
                    }
                    break
                }
                if (attempt < 6) {
                    try { Thread.sleep(350L) }
                    catch (_: InterruptedException) {
                        Thread.currentThread().interrupt()
                        break
                    }
                }
            }
        }
    }

    /** Only cached, bounded, app-private RA PNGs; never fetch art on UI thread. */
    private fun loadValidatedBadge(path: String): Bitmap? = runCatching {
        val safeRoot = File(activity.filesDir, "Saves/RetroAchievements").canonicalFile
        val file = File(path).canonicalFile
        if (!file.path.startsWith(safeRoot.path + File.separator) ||
            file.parentFile?.name != "badges" ||
            !file.name.matches(Regex("[A-Za-z0-9_]{1,64}\\.png")) ||
            !file.isFile || file.length() !in 1L..(128L * 1024)
        ) return@runCatching null
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        if (bounds.outWidth !in 1..256 || bounds.outHeight !in 1..256) return@runCatching null
        BitmapFactory.decodeFile(file.path)
    }.getOrNull()

    fun destroy() {
        destroyed = true
        ++serial
        queue.clear()
        currentNotice = null
        handler.removeCallbacksAndMessages(null)
        card.animate().cancel()
        card.visibility = View.GONE
        loader.shutdownNow()
    }
}
