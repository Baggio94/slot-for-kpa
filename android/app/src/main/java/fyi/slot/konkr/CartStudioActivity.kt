package fyi.slot.konkr

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.graphics.Color
import android.os.Bundle
import android.util.Log
import android.webkit.ValueCallback
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.graphics.drawable.GradientDrawable
import android.widget.LinearLayout
import android.webkit.ConsoleMessage
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import android.widget.TextView
import android.widget.Toast
import java.io.ByteArrayInputStream
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Original Cart Studio, offline HTML/JS/WASM served under a private HTTPS origin.
 * Uses a restricted indexed-library SAF bridge, not a browser SD-card picker.
 */
class CartStudioActivity : Activity() {
    companion object {
        const val EXTRA_ROM = "cart_studio_rom_uri"
        const val RESULT_CHANGED = Activity.RESULT_OK
        private const val ORIGIN = "https://appassets.androidplatform.net"
        private const val PAGE = "$ORIGIN/studio/index.html"
        private const val TAG = "SlotCartStudio"
        private const val PICK_CUSTOM_PNG = 4709
    }

    private val worker = Executors.newSingleThreadExecutor()
    private val changed = AtomicBoolean(false)
    private var web: WebView? = null
    private var singleCartMode = false
    private var pendingPngPicker: ValueCallback<Array<Uri>>? = null

    private fun immersive() {
        // FLAG_FULLSCREEN removes the status-bar window (the grey strip seen
        // on KONKR), while immersive keeps the navigation area out of the UI.
        window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
        window.statusBarColor = Color.rgb(27, 27, 33)
        window.navigationBarColor = Color.BLACK
        window.decorView.systemUiVisibility = (
            View.SYSTEM_UI_FLAG_FULLSCREEN or
            View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
            View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
            View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
            View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
            View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        )
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) immersive()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        immersive()
        val frame = FrameLayout(this)
        frame.setBackgroundColor(SlotUi.background)
        setContentView(frame)
        val notice = TextView(this).apply {
            text = "Cart Studio\nLoading Slot library…"
            typeface = SlotUi.font(this@CartStudioActivity)
            letterSpacing = .09f
            textSize = 18f
            gravity = Gravity.CENTER
            setTextColor(SlotUi.ink)
        }
        frame.addView(notice, FrameLayout.LayoutParams(-1, -1))

        val selected = intent.getStringExtra(EXTRA_ROM)?.takeIf { it.startsWith("content://") }
        singleCartMode = selected != null
        worker.execute {
            try {
                val carts = CartStudioCatalog.games(this)
                require(carts.isNotEmpty()) { "Choose GB, GBC or GBA ROM folders in Library" }
                require(selected == null || carts.any { it.uri == selected }) {
                    "Selected cartridge is no longer in the library"
                }
                // X is an editor for ONE ROM, not a full catalog scan.
                // Menu -> Cart Studio still sees all GB/GBC/GBA games.
                val studioCarts = if (selected != null) carts.filter { it.uri == selected } else carts
                val bridge = CartStudioBridge(this, studioCarts, selected, changed)
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) openStudio(frame, notice, bridge)
                }
            } catch (error: Exception) {
                Log.e(TAG, "Cannot prepare Cart Studio", error)
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) {
                        notice.text = "Unable to load library\n" +
                            (error.message ?: "Check your ROM folder permission")
                    }
                }
            }
        }
    }

    private fun openStudio(frame: FrameLayout, notice: View, bridge: CartStudioBridge) {
        val w = WebView(this)
        web = w
        w.setBackgroundColor(Color.rgb(27, 27, 33))
        w.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            javaScriptCanOpenWindowsAutomatically = false
            setSupportMultipleWindows(false)
            mixedContentMode = android.webkit.WebSettings.MIXED_CONTENT_NEVER_ALLOW
        }
        w.isFocusableInTouchMode = true
        w.addJavascriptInterface(bridge, "AndroidStudio")
        val officialArt = CartStudioArtProxy(this)
        val noIntro = CartStudioDatProxy(this)
        w.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(
                view: WebView?, request: WebResourceRequest?
            ): WebResourceResponse? {
                val url = request?.url ?: return null
                if (request.method != "GET" || url.scheme != "https" ||
                    url.host != "appassets.androidplatform.net" ||
                    !url.path.orEmpty().startsWith("/studio/")) return null
                val path = url.path.orEmpty().removePrefix("/studio/")
                // Native same-origin fetch avoids CORS restrictions from the
                // official art CDN when the Studio runs under appassets.
                if (path.startsWith("art/")) {
                    return officialArt.intercept(path.removePrefix("art/"))
                }
                if (path.startsWith("dat/") && path.endsWith(".dat")) {
                    return noIntro.intercept(path.removePrefix("dat/").removeSuffix(".dat"))
                }
                val safe = Regex("[a-zA-Z0-9_.\\-/]+").matches(path) &&
                    !path.split('/').contains("..") && !path.startsWith("/")
                if (!safe) return missing()
                return try {
                    val mime = when {
                        path.endsWith(".html") -> "text/html"
                        path.endsWith(".js") -> "text/javascript"
                        path.endsWith(".css") -> "text/css"
                        path.endsWith(".wasm") -> "application/wasm"
                        path.endsWith(".json") -> "application/json"
                        path.endsWith(".png") -> "image/png"
                        path.endsWith(".svg") -> "image/svg+xml"
                        else -> return missing()
                    }
                    WebResourceResponse(mime, if (mime.startsWith("text/")) "UTF-8" else null,
                        assets.open("studio/$path"))
                } catch (_: Exception) { missing() }
            }
            override fun shouldOverrideUrlLoading(
                view: WebView?, request: WebResourceRequest?
            ): Boolean {
                // Never let a frame with the JavaScript bridge navigate to a remote page.
                return request?.url?.toString() != PAGE
            }
        }
        w.webChromeClient = object : WebChromeClient() {
            override fun onShowFileChooser(
                webView: WebView?,
                filePathCallback: ValueCallback<Array<Uri>>?,
                fileChooserParams: WebChromeClient.FileChooserParams?,
            ): Boolean {
                if (filePathCallback == null) return false
                // Original Studio's custom logo/label input needs Android's
                // document picker; no broad storage permission is requested.
                pendingPngPicker?.onReceiveValue(null)
                pendingPngPicker = filePathCallback
                val pick = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "image/png"
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                return try {
                    @Suppress("DEPRECATION")
                    startActivityForResult(pick, PICK_CUSTOM_PNG)
                    true
                } catch (error: Exception) {
                    Log.w(TAG, "Unable to open PNG picker", error)
                    pendingPngPicker?.onReceiveValue(null)
                    pendingPngPicker = null
                    false
                }
            }

            override fun onConsoleMessage(message: ConsoleMessage?): Boolean {
                if (message != null) Log.d(TAG, "Studio: " + message.message())
                return true
            }
        }
        frame.removeView(notice)
        // Keep native navigation OUTSIDE the WebView: it cannot cover the
        // original logo, label preview, or Cart Studio controls.
        val barHeight = (48 * resources.displayMetrics.density + 0.5f).toInt()
        val webLayout = FrameLayout.LayoutParams(-1, -1).apply {
            topMargin = barHeight
        }
        frame.addView(w, webLayout)
        val toolbar = FrameLayout(this).apply {
            setBackgroundColor(Color.rgb(27, 27, 33))
        }
        frame.addView(toolbar, FrameLayout.LayoutParams(-1, barHeight, Gravity.TOP))
        val title = SlotUi.label(this, "Cart Studio", 17f, upper = false).apply {
            setTextColor(SlotUi.ink)
            gravity = Gravity.CENTER
        }
        // Absolute center of the complete 960px screen, unaffected by the
        // independent widths of the back button and right-side action.
        // A bounded title area prevents a long translated label overlapping
        // the surrounding controls.
        val titleWidth = minOf(
            (resources.displayMetrics.widthPixels * .28f).toInt(),
            (240 * resources.displayMetrics.density + .5f).toInt()
        )
        title.isSingleLine = true
        title.ellipsize = android.text.TextUtils.TruncateAt.END
        toolbar.addView(title, FrameLayout.LayoutParams(
            titleWidth, -1, Gravity.CENTER
        ))
        val close = SlotUi.nativeHint(this, "B", "BACK").apply {
            setOnClickListener { backInStudio() }
            isFocusable = true
            contentDescription = "Back to Cart Studio library or Slot"
        }
        val backWidth = (116 * resources.displayMetrics.density + 0.5f).toInt()
        toolbar.addView(close, FrameLayout.LayoutParams(backWidth, -1, Gravity.START))
        // Native header actions stay visible as the shelf scrolls. The
        // original Studio owns their enabled state and action semantics.
        val density = resources.displayMetrics.density
        // Only ONE action is visible: Save (dirty) takes priority, otherwise
        // Fill (unlabelled available). Both share the exact same right-aligned
        // slot, so appearance/disappearance never moves the centered title.
        val toolbarButtons = FrameLayout(this).apply {
            setPadding(0, 0, (10 * density).toInt(), 0)
        }
        fun action(label: String, pale: Boolean): TextView {
            return SlotUi.label(this, label, 12f, upper = false).apply {
                gravity = Gravity.CENTER
                setTextColor(if (pale) SlotUi.background else SlotUi.ink)
                setPadding((12 * density).toInt(), 0, (12 * density).toInt(), 0)
                val fillColor = if (pale) SlotUi.ink else SlotUi.surface
                background = SlotUi.plate(this@CartStudioActivity, fillColor)
                setOnFocusChangeListener { view, focused ->
                    view.background = SlotUi.plate(
                        this@CartStudioActivity,
                        if (focused) SlotUi.selected else fillColor, focused
                    )
                    setTextColor(
                        if (focused || !pale) SlotUi.ink else SlotUi.background
                    )
                }
                // Keep measured widths even when hidden; prevent action jumps.
                visibility = View.INVISIBLE
                isFocusable = true
            }
        }
        val fill = action("Fill unlabelled", false)
        val save = action("Save to slot.", true)
        val actionLayout = FrameLayout.LayoutParams(
            -2, (34 * density).toInt(), Gravity.END or Gravity.CENTER_VERTICAL
        )
        toolbarButtons.addView(fill, actionLayout)
        toolbarButtons.addView(save, FrameLayout.LayoutParams(
            -2, (34 * density).toInt(), Gravity.END or Gravity.CENTER_VERTICAL
        ))
        toolbar.addView(toolbarButtons, FrameLayout.LayoutParams(
            -2, -1, Gravity.END or Gravity.CENTER_VERTICAL
        ))
        fill.setOnClickListener {
            w.evaluateJavascript("document.getElementById('fill-open')?.click()", null)
        }
        save.setOnClickListener {
            if (!save.isEnabled) return@setOnClickListener
            save.isEnabled = false
            save.text = "Saving…"
            w.evaluateJavascript("document.getElementById('write')?.click()", null)
        }
        w.addJavascriptInterface(object {
            @JavascriptInterface
            fun progress(done: Int, total: Int) {
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) {
                        save.text = if (total > 0) "Saving ${done.coerceIn(0, total)}/$total"
                                    else "Save to slot."
                        save.isEnabled = total <= 0
                    }
                }
            }

            @JavascriptInterface
            fun summary(written: Int, skipped: Int, failed: Int, shells: Int, shellNote: String) {
                runOnUiThread {
                    if (isFinishing || isDestroyed) return@runOnUiThread
                    val saved = written.coerceAtLeast(0)
                    val skippedSafe = skipped.coerceAtLeast(0)
                    val failedSafe = failed.coerceAtLeast(0)
                    val shellsSafe = shells.coerceAtLeast(0)
                    val parts = mutableListOf<String>()
                    if (saved > 0) parts.add("$saved labels saved")
                    if (shellsSafe > 0) parts.add("$shellsSafe shell changes saved")
                    if (skippedSafe > 0) parts.add("$skippedSafe skipped")
                    if (failedSafe > 0) parts.add("$failedSafe failed")
                    if (shellNote.isNotBlank()) parts.add("shell save failed")
                    val message = parts.joinToString(" · ").ifBlank { "Nothing to save." }
                    val hasWarning = skippedSafe > 0 || failedSafe > 0 || shellNote.isNotBlank()
                    SlotToast.makeText(this@CartStudioActivity, message,
                        if (hasWarning) Toast.LENGTH_LONG else Toast.LENGTH_SHORT).show()
                }
            }

            @JavascriptInterface
            fun actions(canSave: Boolean, canFill: Boolean) {
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) {
                        save.visibility = if (canSave) View.VISIBLE else View.INVISIBLE
                        // X from carousel edits exactly one cart; never flash
                        // the catalog-wide Fill unlabelled action in this mode.
                        fill.visibility = if (!singleCartMode && !canSave && canFill)
                            View.VISIBLE else View.INVISIBLE
                    }
                }
            }
        }, "AndroidToolbar")
        val rule = View(this).apply { setBackgroundColor(SlotUi.rule) }
        toolbar.addView(rule, FrameLayout.LayoutParams(-1,
            (resources.displayMetrics.density + 0.5f).toInt().coerceAtLeast(1),
            Gravity.BOTTOM))
        w.loadUrl(PAGE)
    }

    private fun missing(): WebResourceResponse =
        WebResourceResponse("text/plain", "UTF-8",
            ByteArrayInputStream("Not found".toByteArray(Charsets.UTF_8)))

    @Deprecated("Android 12 document picker result")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == PICK_CUSTOM_PNG) {
            val callback = pendingPngPicker
            pendingPngPicker = null
            val uri = if (resultCode == RESULT_OK) data?.data else null
            callback?.onReceiveValue(
                if (uri?.scheme == "content") arrayOf(uri) else null
            )
            immersive()
        }
    }

    /** In full Studio, B returns from an editor to the cartridge catalog.
     *  At catalog level (or in X's one-cart shortcut), B exits to Slot.
     *  The original editor close button preserves in-memory edits for Save.
     */
    private fun backInStudio() {
        if (isFinishing) return
        if (singleCartMode) { finish(); return }
        val view = web ?: run { finish(); return }
        view.evaluateJavascript("""
            (() => {
                const editor = document.getElementById('editor');
                if (editor && !editor.hidden) {
                    document.getElementById('ed-close')?.click();
                    return 'editor';
                }
                return 'catalog';
            })()
        """.trimIndent()) { result ->
            if (result == "\"catalog\"" && !isFinishing) finish()
        }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        when (keyCode) {
            KeyEvent.KEYCODE_BUTTON_B, KeyEvent.KEYCODE_BACK -> {
                if (event.repeatCount == 0) backInStudio()
                return true
            }
            KeyEvent.KEYCODE_BUTTON_A -> {
                // Preserve the simple A=activate convention used before
                // the experimental full controller editor adapter.
                return web?.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN,
                    KeyEvent.KEYCODE_DPAD_CENTER)) ?: super.onKeyDown(keyCode, event)
            }
        }
        return super.onKeyDown(keyCode, event)
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BUTTON_B || keyCode == KeyEvent.KEYCODE_BACK)
            return true
        if (keyCode == KeyEvent.KEYCODE_BUTTON_A) {
            return web?.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_UP,
                KeyEvent.KEYCODE_DPAD_CENTER)) ?: true
        }
        return super.onKeyUp(keyCode, event)
    }

    override fun finish() {
        if (changed.get()) setResult(RESULT_CHANGED)
        super.finish()
        SlotTransitions.close(this)
    }

    override fun onDestroy() {
        pendingPngPicker?.onReceiveValue(null)
        pendingPngPicker = null
        web?.apply {
            stopLoading()
            removeJavascriptInterface("AndroidStudio")
            removeJavascriptInterface("AndroidToolbar")
            destroy()
        }
        web = null
        worker.shutdownNow()
        // Persist the final (<12 carts) CRC batch for fast subsequent launches.
        try { CartStudioCatalog.flush(this) }
        catch (error: Exception) { Log.w(TAG, "Could not flush Studio CRC cache", error) }
        super.onDestroy()
    }
}
