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

/**
 * Library scan feedback in the shared transparent status rail. Visibility
 * is GONE when idle so the other task can take its position automatically.
 */
internal class SlotLibraryRefreshIndicator(context: Context, rail: LinearLayout) {
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
    private val spinner = object : View(context) {
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = dim(2).toFloat()
            strokeCap = Paint.Cap.ROUND
            color = Color.WHITE
        }
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            if (root.visibility != View.VISIBLE) return
            val pad = paint.strokeWidth + 1f
            canvas.drawArc(RectF(pad, pad, width - pad, height - pad),
                (SystemClock.uptimeMillis() / 4L % 360L).toFloat(),
                265f, false, paint)
            postInvalidateDelayed(40L)
        }
    }
    private val caption: TextView =
        SlotUi.label(context, "UPDATING LIBRARY", 9f, Color.WHITE)

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

    fun show() {
        root.visibility = View.VISIBLE
        spinner.invalidate()
    }
    fun hide() { root.visibility = View.GONE }
}
