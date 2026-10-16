package fyi.slot.konkr

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.bluetooth.BluetoothManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.roundToInt

/**
 * Upper status indicators that follow the exact native Slot footer lifecycle.
 *
 * No package polling, Wi-Fi queries or network IO occurs on the GL thread.
 * Asset bitmaps are the user's approved 24px v6 sprites, decoded only once.
 * The Wi-Fi sprite is generated locally because v6 explicitly omits Wi-Fi.
 *
 * - No radios disabled indicators.
 * - No uninstalled app indicator.
 * - No RA indicator when the RA account/service is disabled.
 * - BasicSync uses its supported STATE_CHANGED / REQUEST_STATE contract.
 * - Syncthing uses its local health endpoint off the UI and GL threads.
 * - SleepManager exposes no third-party status API currently; the positive
 *   running-service observation is best effort on Android 12 and MUST NOT be
 *   mistaken for a guaranteed state check.
 */
internal class SlotStatusIndicators(
    private val host: Activity,
    frame: FrameLayout
) {
    private val scale = (host.resources.displayMetrics.widthPixels / 720f).coerceIn(1f, 2f)
    private val iconSize = (24f * scale).roundToInt()
    private val spacing = (7f * scale).roundToInt()
    private val margin = (24f * scale).roundToInt()
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
    private val sleep = sprite()
    private val basicsync = sprite()
    private val syncthing = sprite()
    private val ra = sprite()
    private val wifi = WifiSignal(host)
    private val bluetooth = sprite()
    private val sprites = HashMap<String, android.graphics.Bitmap>()
    private var statusAtlas: android.graphics.Bitmap? = null
    private val spriteSize = 64
    private val spriteNames = listOf(
        "sleepmanager_running", "sleepmanager_stopped",
        "basicsync_running", "basicsync_stopped",
        "syncthing_running", "syncthing_stopped",
        "raonline", "raoffline", "bluetooth_on"
    )
    private val worker = Executors.newSingleThreadExecutor { task ->
        Thread(task, "slot-status-health").apply {
            priority = Thread.MIN_PRIORITY
            isDaemon = true
        }
    }
    private val healthBusy = AtomicBoolean(false)
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
    private var observedSleepRunning = false
    private var installedBasic = false
    private var installedSyncthing = false
    private var bluetoothPermissionAsked = false
    private var subscribed = false

    private fun sprite(): ImageView = ImageView(host).apply {
        scaleType = ImageView.ScaleType.FIT_CENTER
        isClickable = false
        isFocusable = false
    }
    private fun add(layout: LinearLayout, child: View) {
        layout.addView(child, LinearLayout.LayoutParams(iconSize, iconSize).apply {
            rightMargin = spacing
        })
    }

    init {
        // VectorDrawable is an exact unbadged trace from the approved v6
        // trophy. No online/offline sprite is selected at runtime.
        ra.setImageResource(R.drawable.ic_slot_ra_trophy)
        add(left, sleep)
        add(left, basicsync)
        add(left, syncthing)
        add(left, ra)
        add(right, wifi)
        add(right, bluetooth)
        frame.addView(left, FrameLayout.LayoutParams(-2, iconSize, Gravity.TOP or Gravity.START).apply {
            leftMargin = margin
            topMargin = margin
        })
        frame.addView(right, FrameLayout.LayoutParams(-2, iconSize, Gravity.TOP or Gravity.END).apply {
            rightMargin = margin - spacing
            topMargin = margin
        })
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
        } catch (_: RuntimeException) {
            subscribed = false
        }
    }

    fun onPause() {
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
        observedSleepRunning = sleepObservedRunning()
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
        // On modern Android this is sometimes restricted to own processes.
        // A negative result is NOT authoritative; until SleepManager exposes
        // a signed read-only status API, this is only a best-effort indicator.
        return try {
            val mgr = host.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            @Suppress("DEPRECATION")
            mgr.getRunningServices(100).any {
                it.service.packageName == "com.med.sleepmanager" &&
                    it.service.className.endsWith("SleepManagerService")
            }
        } catch (_: RuntimeException) { false }
    }

    private fun sprite(name: String, view: ImageView) {
        val bitmap = sprites.getOrPut(name) {
            // Approved v6 artwork at native 64x64 instead of upscaled 24px
            // sprites; new RA online/offline badges replace Play/Stop.
            val atlas = statusAtlas ?: host.assets.open("status/status_sprites_hd.png")
                .use { requireNotNull(BitmapFactory.decodeStream(it)) }
                .also { statusAtlas = it }
            val index = spriteNames.indexOf(name)
            require(index >= 0) { "Unknown status sprite: $name" }
            android.graphics.Bitmap.createBitmap(atlas,
                index * spriteSize, 0, spriteSize, spriteSize)
        }
        view.setImageBitmap(bitmap)
    }

    private fun show(view: ImageView, name: String, visible: Boolean) {
        val next = if (visible) View.VISIBLE else View.GONE
        if (view.visibility != next) view.visibility = next
        if (visible && view.tag != name) {
            sprite(name, view)
            view.tag = name
        }
    }

    /**
     * Called only on the main thread from the existing 100ms Slot pulse.
     * Bitmap generation, filesystem package scans and health probing are
     * throttled; a hidden indicator never triggers extra health traffic.
     */
    fun refresh(flags: Int) {
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
            show(sleep, if (observedSleepRunning) "sleepmanager_running"
                else "sleepmanager_stopped", installedSleep)
            show(basicsync, if (basicRunning && basicStateKnown &&
                    now - lastBasicResponse < 45000L) "basicsync_running"
                else "basicsync_stopped", installedBasic)
            // BasicSync and Syncthing-Fork may expose a local Syncthing
            // daemon on the same default port. A healthy port owned by
            // BasicSync is NOT proof the Fork itself is running.
            // When BasicSync is also installed, its own daemon may own
            // port 8384. Attribute that health check to Syncthing-Fork
            // only after BasicSync has explicitly reported NOT RUNNING.
            val forkHealthAttributable = !installedBasic ||
                (basicStateKnown && !basicRunning &&
                    now - lastBasicResponse < 45000L)
            show(syncthing, if (syncthingRunning && forkHealthAttributable)
                "syncthing_running" else "syncthing_stopped", installedSyncthing)
            // Only the account's configured/unconfigured state matters here.
            // Display the SAME unbadged trophy online and offline.
            ra.visibility = if (raAccountConfigured) View.VISIBLE else View.GONE
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
            show(bluetooth, "bluetooth_on", btOn)
        } else {
            wifi.visibility = View.GONE
            bluetooth.visibility = View.GONE
        }
        left.visibility = if (listOf(sleep, basicsync, syncthing, ra)
            .any { it.visibility == View.VISIBLE }) View.VISIBLE else View.GONE
        right.visibility = if (wifi.visibility == View.VISIBLE ||
            bluetooth.visibility == View.VISIBLE) View.VISIBLE else View.GONE
    }

    private class WifiSignal(context: Context) : View(context) {
        var level = 0
            set(value) { if (field != value) { field = value; invalidate() } }
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.rgb(252, 252, 252)
            style = Paint.Style.STROKE
            strokeWidth = 2.6f
            strokeCap = Paint.Cap.ROUND
        }
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            canvas.save()
            canvas.scale(width / 24f, height / 24f)
            val centerX = 12f
            val centerY = 21f
            for (i in 1..3) {
                paint.alpha = if (i <= level) 255 else 70
                val radius = i * 5.7f
                canvas.drawArc(RectF(centerX - radius, centerY - radius,
                    centerX + radius, centerY + radius), 226f, 88f, false, paint)
            }
            paint.style = Paint.Style.FILL
            paint.alpha = if (level > 0) 255 else 70
            canvas.drawCircle(centerX, centerY - 0.9f, 2f, paint)
            paint.style = Paint.Style.STROKE
            canvas.restore()
        }
    }
}
