package fyi.slot.konkr

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.database.ContentObserver
import android.os.Handler
import android.os.Looper
import android.bluetooth.BluetoothManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.SystemClock
import android.util.Log
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import androidx.core.graphics.PathParser
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.roundToInt

/**
 * Upper status indicators that follow the exact native Slot footer lifecycle.
 *
 * No package polling, Wi-Fi queries or network IO occurs on the GL thread.
 * Every glyph is a vector line icon drawn with the weight of the footer
 * battery and clock; a stopped integration is dimmed instead of badged.
 *
 * - No radios disabled indicators.
 * - No uninstalled app indicator.
 * - No RA indicator when the RA account/service is disabled.
 * - BasicSync uses its supported STATE_CHANGED / REQUEST_STATE contract.
 * - Syncthing uses its local health endpoint off the UI and GL threads.
 * - SleepManager 0.7.3+ exposes a read-only ContentProvider Status API v1.
 *   Older versions without that API retain the previous best-effort fallback.
 */
internal class SlotStatusIndicators(
    private val host: Activity,
    frame: FrameLayout
) {
    private val scale = (host.resources.displayMetrics.widthPixels / 720f).coerceIn(1f, 2f)
    private val iconSize = (24f * scale).roundToInt()
    private val spacing = (5f * scale).roundToInt()
    private val margin = (24f * scale).roundToInt()
    // Move only the shelf status row up; preserve its horizontal padding.
    private val statusTop = (12f * scale).roundToInt()
    private val left = LinearLayout(host).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        visibility = View.GONE
        isClickable = false
        isFocusable = false
    }
    private val right = LinearLayout(host).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        visibility = View.GONE
        isClickable = false
        isFocusable = false
    }
    private val sleep = StatusGlyph(host, StatusGlyph.MOON)
    private val basicsync = StatusGlyph(host, StatusGlyph.SYNC)
    private val syncthing = StatusGlyph(host, StatusGlyph.SYNCTHING)
    private val ra = StatusGlyph(host, StatusGlyph.TROPHY)
    private val wifi = StatusGlyph(host, StatusGlyph.WIFI)
    private val bluetooth = StatusGlyph(host, StatusGlyph.BLUETOOTH)
    private val worker = Executors.newSingleThreadExecutor { task ->
        Thread(task, "slot-status-health").apply {
            priority = Thread.MIN_PRIORITY
            isDaemon = true
        }
    }
    private val healthBusy = AtomicBoolean(false)
    private val sleepStatusBusy = AtomicBoolean(false)
    private val wifiManager = host.applicationContext.getSystemService(Context.WIFI_SERVICE)
        as WifiManager
    private val connections = host.getSystemService(Context.CONNECTIVITY_SERVICE)
        as ConnectivityManager
    private val bluetoothManager = host.getSystemService(Context.BLUETOOTH_SERVICE)
        as BluetoothManager
    @Volatile private var syncthingRunning = false
    @Volatile private var basicRunning = false
    @Volatile private var basicStateKnown = false
    private var lastStatePoll = 0L
    private var lastHealthPoll = 0L
    private var lastBasicRequest = 0L
    private var lastBasicResponse = 0L
    private var raAccountConfigured = false
    private var installedSleep = false
    @Volatile private var observedSleepRunning = false
    @Volatile private var sleepApiKnown = false
    private var lastSleepStatusQuery = 0L
    private var sleepStatusObserver: ContentObserver? = null
    private var installedBasic = false
    private var installedSyncthing = false
    private var bluetoothPermissionAsked = false
    private var subscribed = false

    private fun add(layout: LinearLayout, child: View) {
        layout.addView(child, LinearLayout.LayoutParams(iconSize, iconSize).apply {
            rightMargin = spacing
        })
    }

    init {
        // All shelf icons remain white in both themes: they sit on the
        // wallpaper, not on the themed footer band.
        add(left, sleep)
        add(left, basicsync)
        add(left, syncthing)
        add(left, ra)
        add(right, wifi)
        add(right, bluetooth)
        frame.addView(left, FrameLayout.LayoutParams(-2, iconSize, Gravity.TOP or Gravity.START).apply {
            leftMargin = margin
            topMargin = statusTop
        })
        frame.addView(right, FrameLayout.LayoutParams(-2, iconSize, Gravity.TOP or Gravity.END).apply {
            rightMargin = margin - spacing
            topMargin = statusTop
        })
    }

    /**
     * Read-only SleepManager events are cheap notifications; actual provider
     * queries are handled by the existing low-priority status worker.
     */
    private fun registerSleepStatusObserver() {
        if (sleepStatusObserver != null) return
        val observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) {
                requestSleepStatus(force = true)
            }
        }
        runCatching {
            host.contentResolver.registerContentObserver(
                SleepManagerStatusClient.URI, false, observer
            )
        }.onSuccess {
            sleepStatusObserver = observer
        }
    }

    private fun requestSleepStatus(force: Boolean = false) {
        if (!installedSleep || !subscribed) return
        val now = SystemClock.elapsedRealtime()
        // API v1 requires no polling. Retry the legacy fallback at a modest
        // interval until a newer SleepManager gets installed.
        if (!force && sleepApiKnown) return
        if (!force && lastSleepStatusQuery != 0L &&
            now - lastSleepStatusQuery < 15000L) return
        if (!sleepStatusBusy.compareAndSet(false, true)) return
        lastSleepStatusQuery = now
        worker.execute {
            try {
                val status = SleepManagerStatusClient.read(host)
                if (status != null) {
                    sleepApiKnown = true
                    observedSleepRunning = status.enabled && status.serviceRunning
                } else {
                    sleepApiKnown = false
                    // SleepManager <=0.7.2 has no status provider.
                    // This negative observation is never authoritative.
                    observedSleepRunning = sleepObservedRunning()
                }
            } catch (error: Exception) {
                Log.w("SlotStatus", "SleepManager status check failed", error)
            } finally {
                sleepStatusBusy.set(false)
            }
        }
    }

    private val basicReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action != "com.chiller3.basicsync.STATE_CHANGED") return
            val state = intent.getStringExtra("run_state") ?: return
            basicRunning = state == "RUNNING"
            basicStateKnown = true
            lastBasicResponse = SystemClock.elapsedRealtime()
        }
    }

    fun onResume() {
        if (subscribed) return
        val filter = IntentFilter("com.chiller3.basicsync.STATE_CHANGED")
        try {
            if (Build.VERSION.SDK_INT >= 33) {
                host.registerReceiver(basicReceiver, filter, Context.RECEIVER_EXPORTED)
            } else {
                @Suppress("DEPRECATION")
                host.registerReceiver(basicReceiver, filter)
            }
            subscribed = true
            lastBasicRequest = 0
            lastStatePoll = 0
            lastSleepStatusQuery = 0L
            sleepApiKnown = false // refresh after returning from background
            registerSleepStatusObserver()
        } catch (_: RuntimeException) {
            subscribed = false
        }
    }

    fun onPause() {
        sleepStatusObserver?.let { observer ->
            runCatching { host.contentResolver.unregisterContentObserver(observer) }
        }
        sleepStatusObserver = null
        left.visibility = View.GONE
        right.visibility = View.GONE
        if (subscribed) {
            runCatching { host.unregisterReceiver(basicReceiver) }
            subscribed = false
        }
    }

    fun close() {
        onPause()
        worker.shutdownNow()
    }

    private fun installed(pkg: String): Boolean = try {
        @Suppress("DEPRECATION")
        host.packageManager.getPackageInfo(pkg, 0)
        true
    } catch (_: PackageManager.NameNotFoundException) { false }
      catch (_: RuntimeException) { false }

    private fun refreshInstalled() {
        // Presence means an account is registered. Network status only belongs
        // in the actual game-launch notice, not on this top-shelf icon.
        raAccountConfigured = RaAccount.configuredUser(host.filesDir) != null
        installedSleep = installed("com.med.sleepmanager")
        if (installedSleep) {
            requestSleepStatus()
        } else {
            observedSleepRunning = false
            sleepApiKnown = false
            lastSleepStatusQuery = 0L
        }
        installedBasic = installed("com.chiller3.basicsync")
        installedSyncthing = listOf(
            "com.github.catfriend1.syncthingfork",
            "com.github.catfriend1.syncthingfork.debug",
            "com.github.catfriend1.syncthingandroid",
            "com.github.catfriend1.syncthingandroid.debug"
        ).any(::installed)
    }

    private fun requestBasic(now: Long) {
        if (!installedBasic || !subscribed || now - lastBasicRequest < 15000L) return
        lastBasicRequest = now
        runCatching {
            host.sendBroadcast(
                Intent("com.chiller3.basicsync.REQUEST_STATE")
                    .setPackage("com.chiller3.basicsync")
            )
        }
    }

    private fun probeSyncthing(now: Long) {
        if (!installedSyncthing || now - lastHealthPoll < 10000L
            || !healthBusy.compareAndSet(false, true)) return
        lastHealthPoll = now
        worker.execute {
            try {
                val connection = (URL("http://127.0.0.1:8384/rest/noauth/health")
                    .openConnection() as HttpURLConnection).apply {
                    connectTimeout = 350
                    readTimeout = 350
                    instanceFollowRedirects = false
                    requestMethod = "GET"
                }
                syncthingRunning = try {
                    connection.responseCode == HttpURLConnection.HTTP_OK
                } finally { connection.disconnect() }
            } catch (_: Exception) {
                syncthingRunning = false
            } finally { healthBusy.set(false) }
        }
    }

    private fun sleepObservedRunning(): Boolean {
        if (!installedSleep) return false
        // Legacy fallback only for SleepManager without the public API.
        // Modern Android can hide other apps' services: negative != disabled.
        return try {
            val mgr = host.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            @Suppress("DEPRECATION")
            mgr.getRunningServices(100).any {
                it.service.packageName == "com.med.sleepmanager" &&
                    it.service.className.endsWith("SleepManagerService")
            }
        } catch (_: RuntimeException) { false }
    }

    private fun show(view: StatusGlyph, visible: Boolean, running: Boolean = true) {
        val next = if (visible) View.VISIBLE else View.GONE
        if (view.visibility != next) view.visibility = next
        view.alpha = if (running) 1f else STOPPED_ALPHA
    }

    /**
     * Called only on the main thread from the existing 100ms Slot pulse.
     * Filesystem package scans and health probing are
     * throttled; a hidden indicator never triggers extra health traffic.
     */
    fun refresh(flags: Int) {
        // Radio and integration glyphs stay pure white on ALL themes.
        // The library/label task captions still follow Slot's native ink.
        val visible = flags and 1 != 0
        if (!visible) {
            left.visibility = View.GONE
            right.visibility = View.GONE
            return
        }
        val integrations = flags and 8 != 0
        val radios = flags and 6 != 0
        val now = SystemClock.elapsedRealtime()
        if (now - lastStatePoll >= 3000L || lastStatePoll == 0L) {
            lastStatePoll = now
            if (integrations) {
                refreshInstalled()
                requestBasic(now)
                probeSyncthing(now)
            }
        }

        if (integrations) {
            show(sleep, installedSleep, observedSleepRunning)
            show(basicsync, installedBasic, basicRunning && basicStateKnown &&
                now - lastBasicResponse < 45000L)
            // BasicSync and Syncthing-Fork may expose a local Syncthing
            // daemon on the same default port. A healthy port owned by
            // BasicSync is NOT proof the Fork itself is running.
            // When BasicSync is also installed, its own daemon may own
            // port 8384. Attribute that health check to Syncthing-Fork
            // only after BasicSync has explicitly reported NOT RUNNING.
            val forkHealthAttributable = !installedBasic ||
                (basicStateKnown && !basicRunning &&
                    now - lastBasicResponse < 45000L)
            show(syncthing, installedSyncthing,
                syncthingRunning && forkHealthAttributable)
            // Only the account's configured/unconfigured state matters here.
            // Display the SAME trophy online and offline.
            show(ra, raAccountConfigured)
        } else {
            for (v in listOf(sleep, basicsync, syncthing, ra)) v.visibility = View.GONE
        }

        if (radios) {
            val wifiEnabled = flags and 2 != 0 && wifiManager.isWifiEnabled
            wifi.visibility = if (wifiEnabled) View.VISIBLE else View.GONE
            if (wifiEnabled) {
                val network = connections.activeNetwork
                val caps = network?.let(connections::getNetworkCapabilities)
                val info = caps?.transportInfo as? WifiInfo
                val rssi = info?.rssi ?: -127
                wifi.level = if (caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) != true
                    || rssi == -127) 0
                    else when {
                        rssi >= -60 -> 4
                        rssi >= -70 -> 3
                        rssi >= -80 -> 2
                        else -> 1
                    }
            }
            val wantBt = flags and 4 != 0
            val btGranted = host.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) ==
                PackageManager.PERMISSION_GRANTED
            if (wantBt && !btGranted && !bluetoothPermissionAsked) {
                bluetoothPermissionAsked = true
                host.requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_CONNECT), 4710)
            }
            val btOn = wantBt && btGranted && try {
                bluetoothManager.adapter?.isEnabled == true
            } catch (_: SecurityException) { false }
            bluetooth.visibility = if (btOn) View.VISIBLE else View.GONE
        } else {
            wifi.visibility = View.GONE
            bluetooth.visibility = View.GONE
        }
        left.visibility = if (listOf(sleep, basicsync, syncthing, ra)
            .any { it.visibility == View.VISIBLE }) View.VISIBLE else View.GONE
        right.visibility = if (wifi.visibility == View.VISIBLE ||
            bluetooth.visibility == View.VISIBLE) View.VISIBLE else View.GONE
    }

    private companion object {
        const val STOPPED_ALPHA = 0.4f
    }
}

/**
 * Line icons on a 24px grid, stroked with the weight of the footer clock and
 * battery. A faint offset shadow keeps the white glyph legible on a pale
 * wallpaper. Neither colour changes when the shelf switches Dark/Light.
 * Only the Wi-Fi glyph has a level: inactive arcs are drawn at 30%.
 */
internal class StatusGlyph(context: Context, private val glyph: Glyph) : View(context) {
    /** Stroked paths, filled paths, and the minimum level each one needs. */
    class Glyph(val strokes: List<Pair<String, Int>>, val fills: List<Pair<String, Int>>) {
        internal val strokePaths = strokes.map { (d, l) -> PathParser.createPathFromPathData(d) to l }
        internal val fillPaths = fills.map { (d, l) -> PathParser.createPathFromPathData(d) to l }
    }

    var level = MAX_LEVEL
        set(value) {
            if (field != value) { field = value; invalidate() }
        }
    private val ink = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }

    init {
        isClickable = false
        isFocusable = false
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save()
        canvas.scale(width / 24f, height / 24f)
        canvas.save()
        canvas.translate(0f, SHADOW_DY)
        layer(canvas, Color.BLACK, SHADOW_ALPHA, STROKE + SHADOW_SPREAD)
        canvas.restore()
        layer(canvas, Color.WHITE, 1f, STROKE)
        canvas.restore()
    }

    private fun layer(canvas: Canvas, color: Int, opacity: Float, stroke: Float) {
        ink.color = color
        ink.strokeWidth = stroke
        fun alpha(minLevel: Int) =
            (255 * opacity * if (level >= minLevel) 1f else INACTIVE).roundToInt()
        ink.style = Paint.Style.STROKE
        for ((path, minLevel) in glyph.strokePaths) {
            ink.alpha = alpha(minLevel)
            canvas.drawPath(path, ink)
        }
        // The shadow of a filled dot is spread like a stroke.
        ink.style = if (color == Color.BLACK) Paint.Style.FILL_AND_STROKE else Paint.Style.FILL
        ink.strokeWidth = stroke - STROKE
        for ((path, minLevel) in glyph.fillPaths) {
            ink.alpha = alpha(minLevel)
            canvas.drawPath(path, ink)
        }
    }

    companion object {
        const val MAX_LEVEL = 4
        private const val STROKE = 1.9f
        private const val SHADOW_DY = 0.9f
        private const val SHADOW_SPREAD = 0.9f
        private const val SHADOW_ALPHA = 0.38f
        private const val INACTIVE = 0.3f

        private fun dot(x: Float, y: Float, r: Float) =
            "M$x ${y - r}a$r $r 0 1 1 0 ${2 * r}a$r $r 0 1 1 0 ${-2 * r}Z"

        val WIFI = Glyph(
            strokes = listOf(
                "M2.99 8.95A13.2 13.2 0 0 1 21.01 8.95" to 3,
                "M5.93 12.09A8.9 8.9 0 0 1 18.07 12.09" to 2,
                "M8.86 15.24A4.6 4.6 0 0 1 15.14 15.24" to 1
            ),
            fills = listOf(dot(12f, 18.6f, 1.7f) to 1)
        )
        val BLUETOOTH = Glyph(
            // Sits on the Wi-Fi dot's baseline rather than the grid centre.
            strokes = listOf("M7.2 9.4L16.6 16.4L12 20V5.6L16.6 9.2L7.2 16.2" to 0),
            fills = emptyList()
        )
        val MOON = Glyph(
            strokes = listOf("M17.9 14.6A7.1 7.1 0 1 1 10.2 5.1A5.6 5.6 0 0 0 17.9 14.6Z" to 0),
            fills = emptyList()
        )
        val SYNC = Glyph(
            strokes = listOf(
                "M5.6 11A6.6 6.6 0 0 1 16.9 7.1L18.4 8.6" to 0,
                "M18.4 4.9V8.6H14.7" to 0,
                "M18.4 13A6.6 6.6 0 0 1 7.1 16.9L5.6 15.4" to 0,
                "M5.6 19.1V15.4H9.3" to 0
            ),
            fills = emptyList()
        )
        val SYNCTHING = Glyph(
            strokes = listOf(
                "M5.4 12a6.6 6.6 0 1 0 13.2 0a6.6 6.6 0 1 0 -13.2 0" to 0,
                "M12 12L12 5.4M12 12L17.7 15.3M12 12L6.3 15.3" to 0
            ),
            fills = listOf(
                dot(12f, 5.4f, 2.1f) to 0, dot(17.7f, 15.3f, 2.1f) to 0,
                dot(6.3f, 15.3f, 2.1f) to 0, dot(12f, 12f, 1.5f) to 0
            )
        )
        val TROPHY = Glyph(
            strokes = listOf(
                "M8 4.8H16V10A4 4 0 0 1 8 10Z" to 0,
                "M8 6.6H5.4V7.6A3 3 0 0 0 8.4 10.6" to 0,
                "M16 6.6H18.6V7.6A3 3 0 0 1 15.6 10.6" to 0,
                "M12 14V17" to 0,
                "M8.6 19.2H15.4" to 0
            ),
            fills = emptyList()
        )
    }
}
