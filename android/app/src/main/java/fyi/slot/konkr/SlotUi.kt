package fyi.slot.konkr

import android.content.Context
import android.graphics.BitmapFactory
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.ImageView
import android.widget.TextView
import java.util.Locale

/**
 * One visual vocabulary for app-owned Android surfaces, taken from Slot's
 * upstream label.ttf (SIL OFL) and its charcoal / cream palette.
 * OS file/permission/IME dialogs and the original Cart Studio web editor
 * intentionally remain owned by Android / upstream Studio.
 */
internal object SlotUi {
    val background: Int = Color.rgb(23, 22, 25)
    // The same dark opening palette used for native Library / Settings.
    val menuBackground: Int = Color.rgb(8, 8, 11)
    val surface: Int = Color.rgb(37, 36, 43)
    val selected: Int = Color.rgb(68, 66, 77)
    val ink: Int = Color.rgb(246, 244, 239)
    val muted: Int = Color.rgb(174, 171, 180)
    val rule: Int = Color.rgb(91, 89, 96)
    val accent: Int = Color.rgb(210, 188, 134)
    val error: Int = Color.rgb(241, 160, 146)

    @Volatile private var cachedFont: Typeface? = null

    fun font(context: Context): Typeface {
        cachedFont?.let { return it }
        val typeface = runCatching {
            Typeface.createFromAsset(context.applicationContext.assets, "label.ttf")
        }.getOrElse { Typeface.create("sans-serif-condensed", Typeface.BOLD) }
        cachedFont = typeface
        return typeface
    }

    fun dp(context: Context, amount: Int): Int =
        (context.resources.displayMetrics.density * amount + 0.5f).toInt()

    fun plate(context: Context, fill: Int = surface, focused: Boolean = false): GradientDrawable =
        GradientDrawable().apply {
            setColor(fill)
            cornerRadius = dp(context, 4).toFloat()
            setStroke(dp(context, if (focused) 2 else 1), if (focused) ink else rule)
        }

    fun label(
        context: Context, value: String, sp: Float = 13f,
        color: Int = ink, upper: Boolean = true,
    ): TextView = TextView(context).apply {
        text = if (upper) value.uppercase(Locale.ROOT) else value
        typeface = font(context)
        textSize = sp
        letterSpacing = .09f
        includeFontPadding = false
        setTextColor(color)
        gravity = Gravity.CENTER_VERTICAL
    }

    /** Native menus paint quick_value_face at (32, 33), with .79
     * geometry scale and .68 alpha, on a 720×480 canvas. Keep the real
     * Rust rasterization and transparent side padding, not Android text.
     */
    fun menuPixel(context: Context, px: Int): Int =
        (px * context.resources.displayMetrics.widthPixels / 720f + .5f).toInt()

    /** Native Slot menu headings start at x=32; entries begin at x=37. */
    fun menuEntryInset(context: Context): Int = menuPixel(context, 5)

    fun menuHeading(context: Context, title: String): View {
        val asset = when (title.uppercase(Locale.ROOT)) {
            "APPS" -> "heading-apps.png"
            "RETROACHIEVEMENTS" -> "heading-ra.png"
            "SAVE CONFLICT" -> "heading-save-conflict.png"
            "SAVE STATE" -> "heading-save-state.png"
            "SIGN OUT" -> "heading-sign-out.png"
            "LINK — SHARE CARTRIDGE" -> "heading-link-consent.png"
            "MULTIPLAYER — DIRECT JOIN" -> "heading-direct-join.png"
            else -> null
        }
        val image = asset?.let { file ->
            runCatching {
                context.assets.open("apps_controls/$file").use {
                    BitmapFactory.decodeStream(it)
                }
            }.getOrNull()
        }
        if (image == null) return label(context, title, 16f, muted)
        return android.widget.FrameLayout(context).apply {
            addView(ImageView(context).apply {
                setImageBitmap(image)
                scaleType = ImageView.ScaleType.FIT_XY
                alpha = .68f
            }, android.widget.FrameLayout.LayoutParams(
                menuPixel(context, (image.width * .79f).toInt()),
                menuPixel(context, (image.height * .79f).toInt()),
                Gravity.START or Gravity.TOP))
        }
    }

    fun button(context: Context, value: String, onClick: () -> Unit): TextView =
        label(context, value, 13f).apply {
            gravity = Gravity.CENTER
            isFocusable = true
            isClickable = true
            minHeight = dp(context, 41)
            setPadding(dp(context, 11), dp(context, 7), dp(context, 11), dp(context, 7))
            background = plate(context)
            setOnFocusChangeListener { view, focused ->
                view.background = plate(context, if (focused) selected else surface, focused)
            }
            setOnClickListener { onClick() }
        }

    /** Controller-key legend with a real Slot-font cream keycap, not Unicode. */
    fun hint(context: Context, key: String, action: String): android.widget.LinearLayout =
        android.widget.LinearLayout(context).apply {
            orientation = android.widget.LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            val cap = label(context, key, 12f, SlotUi.background).apply {
                gravity = Gravity.CENTER
                minWidth = dp(context, 23)
                minHeight = dp(context, 21)
                setPadding(dp(context, 5), 0, dp(context, 5), 0)
                setBackgroundColor(ink)
            }
            addView(cap)
            val name = label(context, action, 12f, muted).apply {
                setPadding(dp(context, 7), 0, 0, 0)
            }
            addView(name)
        }

    /** The same upstream-rastered keycaps used by the native Slot renderer.
     * Assets are generated at APK build-time by export_apps_legend.rs.
     * Retain the label-font approximation only as a safe asset fallback.
     */
    fun nativeHint(context: Context, key: String, action: String): View {
        val assetName = when (key.uppercase(Locale.ROOT) to action.uppercase(Locale.ROOT)) {
            "A" to "SELECT" -> "select.png"
            "B" to "BACK" -> "back.png"
            "START" to "REFRESH" -> "refresh.png"
            "X" to "SIGN OUT" -> "sign-out.png"
            else -> return hint(context, key, action)
        }
        val image = runCatching {
            context.assets.open("apps_controls/$assetName").use { BitmapFactory.decodeStream(it) }
        }.getOrNull() ?: return hint(context, key, action)
        val scale = (context.resources.displayMetrics.widthPixels / 720f)
            .coerceIn(1f, 2f)
        // A centered container allows weighted RA footer cells and larger
        // Cart Studio bars without stretching upstream's exact 24px raster.
        return android.widget.FrameLayout(context).apply {
            contentDescription = "$key $action"
            addView(ImageView(context).apply {
                setImageBitmap(image)
                scaleType = ImageView.ScaleType.FIT_CENTER
                isClickable = false
            }, android.widget.FrameLayout.LayoutParams(
                (image.width * scale).toInt().coerceAtLeast(1),
                (image.height * scale).toInt().coerceAtLeast(1),
                Gravity.CENTER
            ))
        }
    }

    fun field(context: Context, hint: String, password: Boolean = false): EditText =
        EditText(context).apply {
            this.hint = hint
            isSingleLine = true
            inputType = if (password)
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            else
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            // Android changes a password field's font when inputType is set:
            // re-apply upstream label.ttf AFTER masking/IME flags.
            typeface = font(context)
            textSize = 15f
            setTextColor(ink)
            setHintTextColor(muted)
            letterSpacing = .04f
            imeOptions = if (password) EditorInfo.IME_ACTION_DONE
                else EditorInfo.IME_ACTION_NEXT
            setSelectAllOnFocus(false)
            setPadding(dp(context, 13), dp(context, 10),
                dp(context, 13), dp(context, 10))
            minHeight = dp(context, 48)
            background = plate(context)
            setOnFocusChangeListener { view, focused ->
                view.background = plate(context, surface, focused)
            }
        }
}
