package fyi.slot.konkr

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import java.util.Locale

/**
 * Foreground-only label scan feedback. Pure presentation: polling a small
 * immutable worker snapshot, never posting UI work from each scanned ROM.
 * Shares one transparent layout with the library-refresh indicator.
 */
internal class SlotAutoLabelIndicator(context: Context, rail: LinearLayout) {
    private val scale =
        (context.resources.displayMetrics.widthPixels / 720f).coerceIn(1f, 2f)
    private fun dim(n: Int) = (n * scale + .5f).toInt()
    private val root = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dim(3), 0, dim(3), 0)
        isClickable = false
        isFocusable = false
        visibility = View.GONE
    }
    private val caption: TextView =
        SlotUi.label(context, "LABELS 0 SAVED", 9f, Color.WHITE)
    private val spinner = object : View(context) {
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = dim(2).toFloat()
            strokeCap = Paint.Cap.ROUND
            color = Color.WHITE
        }
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            if (root.visibility != View.VISIBLE || visibility != View.VISIBLE) return
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
        rail.addView(root, LinearLayout.LayoutParams(-2, dim(22)).apply {
            bottomMargin = dim(2)
            gravity = Gravity.END
        })
    }

    fun setLightTheme(light: Boolean) {
        val ink = if (light) Color.BLACK else Color.WHITE
        caption.setTextColor(ink)
        if (spinner.paint.color != ink) {
            spinner.paint.color = ink
            spinner.invalidate()
        }
    }

    fun begin() {
        finishAt = 0L
        spinner.visibility = View.VISIBLE
        caption.text = "LABELS 0 SAVED"
        root.visibility = View.VISIBLE
        spinner.invalidate()
    }

    fun progress(checked: Int, total: Int, saved: Int) {
        if (root.visibility != View.VISIBLE) begin()
        caption.text = if (total > 0) {
            String.format(Locale.ROOT, "LABELS %d/%d  SAVED %d",
                checked.coerceIn(0, total), total, saved.coerceAtLeast(0))
        } else {
            String.format(Locale.ROOT, "LABELS SAVED %d", saved.coerceAtLeast(0))
        }
    }

    fun finished(saved: Int) {
        if (root.visibility != View.VISIBLE) return
        spinner.visibility = View.GONE
        caption.text = String.format(Locale.ROOT, "%d LABELS ADDED",
            saved.coerceAtLeast(0))
        finishAt = SystemClock.uptimeMillis() + 1600L
    }

    fun tick() {
        if (finishAt != 0L && SystemClock.uptimeMillis() >= finishAt) hide()
    }

    fun hide() {
        finishAt = 0L
        root.visibility = View.GONE
    }
}
