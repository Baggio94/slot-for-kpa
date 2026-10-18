package fyi.slot.konkr

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import java.util.Locale

/**
 * Non-interactive, foreground-only artwork progress indicator.
 * Shares the existing library spinner's upper-right visual vocabulary,
 * positioned below it so both can be visible without overlapping.
 * All calls are on Android's main thread; never polls ROMs or network.
 */
internal class SlotAutoLabelIndicator(context: Context, frame: FrameLayout) {
    private val scale = (context.resources.displayMetrics.widthPixels / 720f).coerceIn(1f, 2f)
    private fun dim(n: Int) = (n * scale + .5f).toInt()
    private val root = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dim(6), dim(2), dim(6), dim(2))
        setBackgroundColor(SlotUi.menuBackground)
        isClickable = false
        isFocusable = false
        visibility = View.GONE
    }
    private val caption = SlotUi.label(context, "LABELS 0/0", 9f, SlotUi.muted)
    private val spinner = object : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = dim(2).toFloat()
            strokeCap = Paint.Cap.ROUND
            color = SlotUi.ink
        }
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            if (root.visibility != View.VISIBLE) return
            val inset = paint.strokeWidth + 1f
            canvas.drawArc(RectF(inset, inset, width - inset, height - inset),
                (SystemClock.uptimeMillis() / 4L % 360L).toFloat(),
                265f, false, paint)
            postInvalidateDelayed(40L)
        }
    }
    private var finishAt = 0L

    init {
        root.addView(spinner, LinearLayout.LayoutParams(dim(13), dim(13)))
        caption.setPadding(dim(6), 0, 0, 0)
        root.addView(caption)
        frame.addView(root, FrameLayout.LayoutParams(-2, dim(20),
            Gravity.TOP or Gravity.END).apply {
            // Above native status sprites (24px top margin) and the
            // library-refresh banner (59px), never overlap either.
            topMargin = dim(2)
            rightMargin = dim(10)
        })
    }

    fun progress(checked: Int, total: Int) {
        if (total <= 0) { hide(); return }
        finishAt = 0L
        caption.text = String.format(Locale.ROOT, "LABELS %d/%d",
            checked.coerceIn(0, total), total)
        if (spinner.visibility != View.VISIBLE) spinner.visibility = View.VISIBLE
        root.visibility = View.VISIBLE
        spinner.invalidate()
    }

    /** Show only an actual success and auto-hide after a short dwell. */
    fun finished(saved: Int) {
        if (root.visibility != View.VISIBLE) return
        if (saved == 0) { hide(); return }
        caption.text = String.format(Locale.ROOT, "%d LABELS ADDED", saved)
        spinner.visibility = View.GONE
        finishAt = SystemClock.uptimeMillis() + 2000L
    }

    fun tick() {
        if (finishAt != 0L && SystemClock.uptimeMillis() >= finishAt) hide()
    }

    fun hide() {
        finishAt = 0L
        root.visibility = View.GONE
    }
}
