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

/** Tiny upper-right Slot-style spinner shown only for an actual changed index.
 * No animation or extra status overlay for an unchanged boot scan.
 */
internal class SlotLibraryRefreshIndicator(context: Context, frame: FrameLayout) {
    private val scale = (context.resources.displayMetrics.widthPixels / 720f).coerceIn(1f, 2f)
    private fun dim(n: Int) = (n * scale + .5f).toInt()
    private val root = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dim(8), dim(4), dim(8), dim(4))
        setBackgroundColor(SlotUi.menuBackground)
        isClickable = false
        isFocusable = false
        visibility = View.GONE
    }
    private val spinner = object : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = dim(2).toFloat()
            strokeCap = Paint.Cap.ROUND
            color = SlotUi.ink
        }
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            val pad = paint.strokeWidth + 1f
            val bounds = RectF(pad, pad, width - pad, height - pad)
            canvas.drawArc(bounds,
                (SystemClock.uptimeMillis() / 4L % 360L).toFloat(), 265f, false, paint)
            if (root.visibility == View.VISIBLE) postInvalidateDelayed(40L)
        }
    }
    init {
        root.addView(spinner, LinearLayout.LayoutParams(dim(13), dim(13)))
        root.addView(SlotUi.label(context, "UPDATING LIBRARY", 9f, SlotUi.muted).apply {
            setPadding(dim(6), 0, 0, 0)
            isClickable = false
            isFocusable = false
        })
        frame.addView(root, FrameLayout.LayoutParams(-2, dim(26),
            Gravity.TOP or Gravity.END).apply {
            topMargin = dim(59)
            rightMargin = dim(24)
        })
    }
    fun show() {
        root.visibility = View.VISIBLE
        spinner.invalidate()
    }
    fun hide() { root.visibility = View.GONE }
}
