package fyi.slot.konkr

import android.app.Activity
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.media.PlaybackParams
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.os.BatteryManager
import android.os.VibrationEffect
import android.os.Vibrator
import android.content.IntentFilter
import android.util.AtomicFile
import java.security.MessageDigest
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import android.content.Intent
import android.content.ActivityNotFoundException
import android.graphics.Color
import android.graphics.Bitmap
import android.widget.ImageView
import java.nio.ByteBuffer
import java.nio.ByteOrder
import android.graphics.BitmapFactory
import java.io.ByteArrayOutputStream
import android.view.Gravity
import android.widget.FrameLayout
import android.widget.TextView
import android.widget.Toast
import android.widget.EditText
import android.net.Uri
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import java.net.Inet4Address
import org.json.JSONObject
import org.json.JSONArray
import java.util.concurrent.ConcurrentHashMap
import android.provider.DocumentsContract
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import android.view.Choreographer
import android.os.Bundle
import android.util.Log
import android.view.KeyEvent
import android.view.View
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10

/** Android owns EGL, lifecycle and physical input; Rust owns Slot rendering. */
class MainActivity : Activity() {
    companion object {
        init { System.loadLibrary("slot_android") }
        private const val TAG = "SlotKonkr"
        private const val FOLDER_REQUEST = 4701
        private const val BIOS_REQUEST = 4702
        private const val SAVE_REQUEST = 4703
        private const val STATE_REQUEST = 4704
        private const val THEME_REQUEST = 4705
        private const val WALLPAPER_REQUEST = 4706
        private const val LABEL_REQUEST = 4707
        private const val CART_STUDIO_REQUEST = 4708
        private const val PREFS = "slot_konkr_library"
        private const val ROM_ROOT = "rom_root_uri"
        private const val BIOS_ROOT = "bios_root_uri"
        private const val SAVE_ROOT = "saves_root_uri"
        private const val SHARE_GBA_SAVES = "share_gba_saves"
        private const val STATE_ROOT = "states_root_uri"
        private const val ROM_CACHE_NAME = "rom-library-cache-v1.json"
        private const val ROM_CACHE_SCHEMA = 1
        private const val ROM_CACHE_MAX_BYTES = 8 * 1024 * 1024
        private val BUTTONS = setOf(
            KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT,
            KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN,
            KeyEvent.KEYCODE_BUTTON_L1, KeyEvent.KEYCODE_BUTTON_R1,
            KeyEvent.KEYCODE_BUTTON_L2, KeyEvent.KEYCODE_BUTTON_R2,
            KeyEvent.KEYCODE_BUTTON_A, KeyEvent.KEYCODE_BUTTON_B,
            KeyEvent.KEYCODE_BUTTON_X, KeyEvent.KEYCODE_BUTTON_Y,
            KeyEvent.KEYCODE_BUTTON_START, KeyEvent.KEYCODE_BUTTON_SELECT,
            KeyEvent.KEYCODE_BUTTON_MODE
        )
    }

    external fun nativeSurfaceCreated(): Boolean
    external fun nativeSurfaceChanged(width: Int, height: Int)
    external fun nativeDrawFrame()
    external fun nativeKey(code: Int, pressed: Boolean)
    external fun nativeResetInput()
    external fun nativeExternalReturn()
    external fun nativePerformanceStats(): String?
    external fun nativeShelfIndicatorFlags(): Int
    external fun nativeSetShelfStatus(code: Int)
    external fun nativeSetLibrary(json: String): Int
    external fun nativeConfigure(dataDir: String, libraryDir: String)
    external fun nativePollLaunchUri(): String?
    external fun nativeGameReady(uri: String, path: String)
    external fun nativeGameError(uri: String, message: String)
    external fun nativeExitGame()
    external fun nativeSuspend()
    external fun nativeIsPlaying(): Boolean
    external fun nativeCarouselQuiet(): Boolean
    external fun nativeWarmRomCandidate(): String?
    external fun nativeCanRefreshLibrary(): Boolean
    external fun nativeAudioSampleRate(): Int
    external fun nativeAudioQueuedShorts(): Int
    external fun nativeSetMgbaLinkRecoveryEnabled(enabled: Boolean)
    external fun nativeDisplaySyncEnabled(): Boolean
    external fun nativeAudioSpeedPermille(): Int
    external fun nativeFastAudioSupported(supported: Boolean)
    external fun nativeReadAudio(): ShortArray
    external fun nativePollMessage(): String?
    external fun nativePollRaNotice(): String?
    external fun nativeRaStatus(): String?
    external fun nativeRaNetworkAvailable()
    external fun nativeRaAccountConfigured()
    external fun nativeRaPrefetchHash(hash: String, console: Int)
    external fun nativeRaFlushReady(): Boolean
    external fun nativeRaStopReady(): Boolean
    external fun nativeSystemStatus(clock: String, batteryPercent: Int, charging: Boolean)
    external fun nativePollUiAction(): Int
    external fun nativeUpdateStatus(status: Int, installed: String, available: String, automatic: Boolean)
    external fun nativeReloadVisualAssets()
    external fun nativePollCartLabelUri(): String?
    external fun nativeReloadCartLabel(uri: String)
    external fun nativePollCartSfx(): Int
    external fun nativePollRumbleStrength(): Int
    external fun nativeCoreForUri(uri: String): String
    external fun nativePollSaveFlush(): String?
    external fun nativeSetShareSaves(enabled: Boolean)
    external fun nativeSetNextResumeMode(mode: Int)
    // The renderer requests LAN only when Host/Join is selected. It never
    // touches WifiManager, Android UI or permissions from the GL thread.
    external fun nativeLinkPeerRomDecision(choice: Int)
    external fun nativeLinkPeerLabel(): String?
    external fun nativeLinkNetworkReady(ipv4: String)
    external fun nativeLinkNetworkError(reason: String)
    external fun nativeLinkNeedsMulticast(): Boolean

    private var pendingCartLabelUri: String? = null
    private var pendingCartStudioUri: String? = null
    private lateinit var cartSounds: CartSounds
    private val scanner = Executors.newSingleThreadExecutor()
    private data class PendingLibraryIndex(
        val root: Uri, val json: String, val prepared: String,
        val serial: Int, val quiet: Boolean
    )
    private var pendingLibraryIndex: PendingLibraryIndex? = null
    private lateinit var taskIndicators: SlotShelfTaskIndicators
    private lateinit var libraryRefreshIndicator: SlotLibraryRefreshIndicator
    private lateinit var autoLabelIndicator: SlotAutoLabelIndicator
    private val raLoginWorker = Executors.newSingleThreadExecutor()
    private val raSmartWorker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "slot-ra-smart-cache").apply { priority = Thread.MIN_PRIORITY }
    }
    private val raSmartRunning = AtomicBoolean(false)
    private val gameLoader = Executors.newSingleThreadExecutor()
    // Optional I/O must never delay a real ROM launch or pending save exports.
    private val romWarmWorker = Executors.newSingleThreadExecutor { task ->
        Thread(task, "slot-kpa-rom-warm").apply { priority = Thread.MIN_PRIORITY }
    }
    private val warmedCandidate = java.util.concurrent.atomic.AtomicReference<String?>(null)
    private val autoLabelsWorker = Executors.newSingleThreadExecutor { task ->
        Thread(task, "slot-kpa-auto-labels").apply { priority = Thread.MIN_PRIORITY }
    }
    private val autoLabelsRunning = AtomicBoolean(false)
    private val autoLabelsRestartRequested = AtomicBoolean(false)
    @Volatile private var cartStudioOpening = false
    // Paint fresh labels only when all moving carts and pending GPU uploads
    // have settled. The renderer's manual reload path is synchronous.
    private val autoLabelUpdates = java.util.concurrent.ConcurrentLinkedQueue<String>()
    private val activeTargets = ConcurrentHashMap<String, RetroArchStorage.Target>()
    private val uiHandler = Handler(Looper.getMainLooper())
    private val slotUpdater: SlotUpdater by lazy {
        SlotUpdater(this) { status, version ->
            if (!isDestroyed) {
                val automatic = getSharedPreferences("slot-updater-v1", MODE_PRIVATE)
                    .getBoolean("enabled", true)
                nativeUpdateStatus(status,
                    packageManager.getPackageInfo(packageName, 0).versionName.orEmpty(),
                    version, automatic)
            }
        }
    }
    private val audioRunning = AtomicBoolean(false)
    // A single pending wakeup is enough. GLSurfaceView never waits for audio.
    // Prevent an empty-read 8ms sleep from missing a freshly produced frame.
    private val audioFrameSignal = java.util.concurrent.Semaphore(0)
    private val vibrator by lazy { getSystemService(VIBRATOR_SERVICE) as? Vibrator }
    private var lastRumbleAt = 0L
    @Volatile private var resumed = false
    private val raSmartForeground = java.lang.Object()
    private var carouselDozing = false
    private var lastCarouselHeartbeat = 0L
    private var returnFromSlotChild = false
    private lateinit var performanceHud: TextView
    private lateinit var shelfStatus: SlotStatusIndicators
    private var lastPerfPoll = 0L
    private var displaySyncApplied = false
    // RA cached game details can be ready before Slot's seating/CRT sequence
    // has set PLAYING=true. Hold that notice, then reveal it on actual gameplay.
    // Distinguish that legitimate startup from a stale notice after eject.
    private var raLaunchPending = false
    private var raLaunchRequestedAt = 0L
    private var raWasPlaying = false
    private var raPendingGameNotice: RaAchievementOverlay.Notice? = null

    // Only attach the Choreographer callback while DISPLAY SYNC is selected.
    // Normal RESPONSIVE and STANDARD add no extra work to every UI vsync.
    private val vsyncCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!displaySyncApplied || !resumed || isDestroyed) return
            view.requestRender()
            Choreographer.getInstance().postFrameCallback(this)
        }
    }
    private fun updateDisplaySyncMode() {
        if (!::view.isInitialized) return
        val enabled = resumed && nativeIsPlaying() && nativeDisplaySyncEnabled()
        if (enabled == displaySyncApplied) return
        displaySyncApplied = enabled
        if (enabled) {
            view.renderMode = GLSurfaceView.RENDERMODE_WHEN_DIRTY
            Choreographer.getInstance().postFrameCallback(vsyncCallback)
        } else {
            Choreographer.getInstance().removeFrameCallback(vsyncCallback)
            view.renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
        }
    }

    // The engine still runs every frame while animating, scrolling, playing,
    // or preparing cartridge art. A settled shelf is refreshed at 4 Hz for
    // clocks, library scans and asynchronous UI changes.
    private fun wakeCarouselRender() {
        if (!::view.isInitialized || !resumed) return
        if (carouselDozing) {
            carouselDozing = false
            view.renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
        }
        view.requestRender()
    }

    private fun updateCarouselPacing() {
        if (!::view.isInitialized || !resumed) return
        val canDoze = !displaySyncApplied && !nativeIsPlaying() && nativeCarouselQuiet()
        if (!canDoze) {
            if (carouselDozing) wakeCarouselRender()
            return
        }
        if (!carouselDozing) {
            carouselDozing = true
            lastCarouselHeartbeat = SystemClock.uptimeMillis()
            view.renderMode = GLSurfaceView.RENDERMODE_WHEN_DIRTY
        }
        val now = SystemClock.uptimeMillis()
        if (now - lastCarouselHeartbeat >= 250L) {
            lastCarouselHeartbeat = now
            view.requestRender()
        }
    }

    private fun publishAutoLabelsWhenIdle() {
        if (nativeIsPlaying() || !nativeCarouselQuiet()) return
        // One face per UI pulse, never a burst on the GL owner.
        val uri = autoLabelUpdates.poll() ?: return
        nativeReloadCartLabel(uri)
        wakeCarouselRender()
    }

    private fun updateRomWarmCandidate() {
        val next = if (nativeIsPlaying()) null else nativeWarmRomCandidate()
        if (warmedCandidate.getAndSet(next) == next || next == null) return
        try {
            romWarmWorker.execute {
                RomWarmCache.prepare(applicationContext, next) {
                    resumed && !nativeIsPlaying() && warmedCandidate.get() == next
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) { /* shutting down */ }
    }

    private var audioThread: Thread? = null
    private var raConnectivity: ConnectivityManager? = null
    private var activeRaPanel: Pair<RaPanelDialog, String>? = null
    private fun refreshRaConnectivityOnScreen() {
        // ConnectivityManager may call us from a binder thread: touch views
        // only on main, and reuse the already scanned account snapshot.
        runOnUiThread {
            val (panel, user) = activeRaPanel ?: return@runOnUiThread
            if (panel.isShowing()) {
                RaDiagnostics.lastSnapshot(user)?.let { snapshot ->
                    panel.update(retroAchievementsStatus(), hasValidatedInternet(), snapshot)
                }
            }
        }
    }
    private var linkMulticastLock: WifiManager.MulticastLock? = null

    /** The LAN need not have Internet: the two handhelds only need the same Wi-Fi.
     *  Resolve the actual interface's IPv4 address, never use the default
     *  network's mobile/VPN address or a hardcoded 10.42.0.1 Linux radio. */
    private fun linkWifiAddress(): String? {
        val cm = getSystemService(CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return null
        for (network in cm.allNetworks) {
            val capabilities = cm.getNetworkCapabilities(network) ?: continue
            if (!capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) continue
            val links = cm.getLinkProperties(network) ?: continue
            for (address in links.linkAddresses) {
                val ip = address.address
                if (ip is Inet4Address && !ip.isLoopbackAddress &&
                    !ip.isLinkLocalAddress && !ip.isMulticastAddress && !ip.isAnyLocalAddress) {
                    return ip.hostAddress
                }
            }
        }
        return null
    }

    private fun releaseLinkMulticast() {
        val current = linkMulticastLock
        linkMulticastLock = null
        try { if (current?.isHeld == true) current.release() }
        catch (error: RuntimeException) { Log.w(TAG, "Link multicast release failed", error) }
    }

    /** The peer has proven possession of an installation-bound Ed25519 key
     * before this UI appears. A14 never trusts IPs or legacy lab flags.
     * Keys are remembered only by Rust after a verified ROM exchange.
     */
    private fun showPeerCartridgeConsent() {
        if (!resumed || isDestroyed || isFinishing) {
            nativeLinkPeerRomDecision(0)
            return
        }
        val answered = AtomicBoolean(false)
        fun reply(choice: Int) {
            if (answered.compareAndSet(false, true)) nativeLinkPeerRomDecision(choice)
        }
        try {
            val peer = nativeLinkPeerLabel().orEmpty().take(36)
                .ifBlank { "UNRECOGNIZED DEVICE" }
            val dialog = SlotModalDialog(
                this,
                "LINK — SHARE CARTRIDGE",
                "Share this ROM with $peer over local Wi-Fi? " +
                    "Only allow a device you trust.",
                onDismiss = { reply(0) }
            )
            dialog.addButton("CANCEL") { reply(0); dialog.dismiss() }
            dialog.addButton("ALLOW ONCE") { reply(1); dialog.dismiss() }
            dialog.addButton("REMEMBER DEVICE") { reply(2); dialog.dismiss() }
            dialog.show()
        } catch (error: Exception) {
            Log.w(TAG, "Cannot show Slot peer permission", error)
            reply(0)
        }
    }

    private fun requestLinkWifi() {
        if (!resumed) {
            nativeLinkNetworkError("Return to Slot before starting Multiplayer")
            return
        }
        val address = linkWifiAddress()
        if (address == null) {
            nativeLinkNetworkError("Connect both consoles to the same Wi-Fi network")
            return
        }
        try {
            releaseLinkMulticast()
            val manager = applicationContext.getSystemService(WIFI_SERVICE) as WifiManager
            val lock = manager.createMulticastLock("slot-konkr-multiplayer")
            lock.setReferenceCounted(false)
            lock.acquire()
            linkMulticastLock = lock
            // Lock must be held before the Rust discovery worker starts.
            nativeLinkNetworkReady(address)
        } catch (error: Exception) {
            releaseLinkMulticast()
            Log.w(TAG, "Unable to acquire LAN discovery lock", error)
            nativeLinkNetworkError("Wi-Fi multicast unavailable on this device")
        }
    }
    private val raConnectivityCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            // Signals only that a network appeared. Rust still verifies RA HTTPS.
            if (!isDestroyed) {
                nativeRaNetworkAvailable()
                startRaSmartCache()
                startAutoCartLabels()
                refreshRaConnectivityOnScreen()
            }
        }
        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            if (!isDestroyed) {
                if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) {
                    startRaSmartCache()
                    startAutoCartLabels()
                }
                refreshRaConnectivityOnScreen()
            }
        }
        override fun onLost(network: Network) {
            if (!isDestroyed) refreshRaConnectivityOnScreen()
        }
    }
    private val pulse = object : Runnable {
        override fun run() {
            // No 100ms UI supervision after the Activity loses foreground.
            if (!resumed || isDestroyed) return
            // Changed ROM indexes are applied only when the GL engine is
            // back on its idle carousel, never during a game or its launch.
            publishPendingLibraryIfIdle()
            pollSystemStatus()
            if (::shelfStatus.isInitialized) {
                val flags = nativeShelfIndicatorFlags()
                shelfStatus.refresh(flags)
                if (::taskIndicators.isInitialized) {
                    taskIndicators.setLightTheme(flags and 16 != 0)
                }
                // Library feedback and START are rendered inside Slot's
                // GL carousel, not overlaid by an Android TextView.
            }
            pollRumble()
            updateDisplaySyncMode()
            updateCarouselPacing()
            publishAutoLabelsWhenIdle()
            if (::autoLabelIndicator.isInitialized) {
                if (nativeIsPlaying() || cartStudioOpening) {
                    autoLabelIndicator.hide()
                } else if (autoLabelsRunning.get()) {
                    // The scanner only writes one immutable, lock-free
                    // snapshot. No Handler post or UI work per cartridge.
                    val progress = AutoCartLabelDiagnostics.snapshot()
                    if (progress.total > 0) {
                        autoLabelIndicator.progress(
                            progress.checked, progress.total, progress.saved)
                    }
                } else {
                    autoLabelIndicator.tick()
                }
            }
            updateRomWarmCandidate()
            if (resumed && nativeIsPlaying()) {
                if (!audioRunning.get()) startAudio()
            } else if (audioRunning.get()) {
                stopAudio()
            }
            drainPendingSaveFlush()
            val request = nativePollLaunchUri()
            if (request != null) {
                raLaunchPending = true
                raLaunchRequestedAt = SystemClock.elapsedRealtime()
                raPendingGameNotice = null
                loadGameFromSaf(request)
            }
            when (nativePollUiAction()) {
                1 -> openRomFolderPicker()
                2 -> refreshLibrary()
                3 -> {
                    try {
                        // Dialog.show() returns before Android's first
                        // composited frame. The RA dialog's dismiss callback,
                        // not this call, owns closing the native Slot Menu.
                        openRetroAchievementsAccount()
                    } catch (error: Exception) {
                        nativeExternalReturn()
                        Log.e(TAG, "Unable to open RetroAchievements", error)
                        SlotToast.makeText(this@MainActivity,
                            "Unable to open RetroAchievements.", Toast.LENGTH_SHORT).show()
                    }
                }
                4 -> openBiosFolderPicker()
                5 -> openSharedFolderPicker(SAVE_REQUEST)
                6 -> openSharedFolderPicker(STATE_REQUEST)
                7 -> openVisualPicker(THEME_REQUEST)
                8 -> openVisualPicker(WALLPAPER_REQUEST)
                9 -> resetVisualFile(THEME_REQUEST)
                10 -> resetVisualFile(WALLPAPER_REQUEST)
                11 -> openCartLabelPicker()
                12 -> removeSelectedCartLabel()
                13 -> openSelectedCartStudio()
                14 -> openAllCartStudio()
                15 -> {
                    try {
                        returnFromSlotChild = true
                        startActivity(Intent(this@MainActivity, AppsActivity::class.java))
                        SlotTransitions.open(this@MainActivity)
                    } catch (error: Exception) {
                        returnFromSlotChild = false
                        nativeExternalReturn()
                        Log.w(TAG, "Cannot open Apps", error)
                        SlotToast.makeText(this@MainActivity, "Unable to open Apps.",
                            Toast.LENGTH_SHORT).show()
                    }
                }
                16 -> requestLinkWifi()
                17 -> toggleShareGbaSaves()
                18 -> showPeerCartridgeConsent()
                19 -> slotUpdater.check(true)
                20 -> slotUpdater.openNotes()
                21 -> {
                    if (!nativeIsPlaying()) {
                        slotUpdater.downloadAndInstall { apk ->
                            if (!isDestroyed && resumed && !nativeIsPlaying()) {
                                try {
                                    slotUpdater.startInstaller(apk)
                                } catch (error: Exception) {
                                    Log.w(TAG, "User-confirmed installer not available", error)
                                    SlotToast.makeText(this@MainActivity,
                                        "Allow updates from slot. in Android settings.",
                                        Toast.LENGTH_LONG).show()
                                }
                            }
                        }
                    }
                }
                22 -> {
                    val enabled = slotUpdater.toggle()
                    slotUpdater.refreshVisibleStatus()
                    if (enabled) slotUpdater.check(false)
                }
            }
            // Multicast is required during discovery, not for ongoing TCP
            // emulation. Release on success, cancellation and timeout.
            if (linkMulticastLock != null && !nativeLinkNeedsMulticast()) {
                releaseLinkMulticast()
            }
            val nowPerf = android.os.SystemClock.uptimeMillis()
            if (resumed && nowPerf - lastPerfPoll >= 250) {
                lastPerfPoll = nowPerf
                val value = nativePerformanceStats()
                performanceHud.visibility = if (value.isNullOrBlank()) View.GONE else View.VISIBLE
                if (!value.isNullOrBlank() && performanceHud.text.toString() != value) {
                    performanceHud.text = value
                }
            }
            nativePollMessage()?.let { message ->
                // Keep the complete diagnostic in Logcat. Actionable failures
                // receive a longer on-screen warning rather than vanishing
                // after the same interval as a simple confirmation.
                Log.w(TAG, message)
                val concise = SlotToast.userMessage(message)
                val needsAction = listOf(
                    "can't", "cannot", "couldn't", "failed", "unavailable",
                    "paused", "mismatch", "needs", "check", "retry"
                ).any { concise.contains(it, ignoreCase = true) }
                SlotToast.makeText(this@MainActivity, concise,
                    if (needsAction) Toast.LENGTH_LONG else Toast.LENGTH_SHORT).show()
            }
            // RetroArch-like achievement widget: cached official badge and
            // original Slot label font. Foreground-only, in our Activity frame,
            // never a system Toast that Android may place at screen bottom.
            if (resumed) {
                val gameActive = nativeIsPlaying()
                // On a real game start, flush any cached early RA status.
                // Only a PLAYING -> stopped edge means a game was ejected.
                // Earlier Beta19 incorrectly treated loading as ejected.
                if (gameActive && !raWasPlaying) {
                    raLaunchPending = false
                    raPendingGameNotice?.let { raAchievementOverlay.push(it) }
                    raPendingGameNotice = null
                } else if (!gameActive && raWasPlaying) {
                    raLaunchPending = false
                    raPendingGameNotice = null
                    raAchievementOverlay.dismissGameLaunchNotices()
                } else if (!gameActive && raLaunchPending &&
                    SystemClock.elapsedRealtime() - raLaunchRequestedAt > 12_000L) {
                    // Failed or cancelled launches must not preserve a stale
                    // pre-launch notice indefinitely.
                    raLaunchPending = false
                    raPendingGameNotice = null
                }
                raWasPlaying = gameActive
                repeat(4) {
                    val raw = nativePollRaNotice() ?: return@repeat
                    try {
                        val notice = JSONObject(raw)
                        val title = notice.optString("title", "").take(100)
                        val detail = notice.optString("detail", "").take(160)
                        if (title.isNotBlank()) {
                            val earned = notice.optBoolean("earned", false)
                            // This line is only for a recognized game's launch
                            // notification. Never decorate unlock/error notices.
                            val isGameReady = !earned &&
                                detail.startsWith("You have ") &&
                                detail.contains(" achievements unlocked")
                            val uiNotice = RaAchievementOverlay.Notice(
                                title = title,
                                detail = detail,
                                earned = earned,
                                badge = notice.optString("badge", "").takeIf { it.isNotBlank() },
                                onlineMode = if (isGameReady) hasValidatedInternet() else null
                            )
                            if (isGameReady && !gameActive) {
                                // During the cartridge seating/CRT animation,
                                // native RA's cached game can already be ready.
                                // Don't discard that valid launch notification.
                                // But never display an old status after ejection.
                                if (raLaunchPending) raPendingGameNotice = uiNotice
                            } else {
                                raAchievementOverlay.push(uiNotice)
                            }
                        }
                    } catch (_: Exception) {
                        Log.w(TAG, "Ignoring malformed RetroAchievements notice")
                    }
                }
                // Ejection cleanup is edge-triggered above, not performed
                // every idle/loading poll (which suppressed RA in Beta19).
            }
            if (!isDestroyed && resumed) uiHandler.postDelayed(this, 100L)
        }
    }
    // Runs on the main thread, queueing real storage I/O on gameLoader.
    // Called at the 100ms foreground pulse AND after the final suspend save,
    // so stopping the UI timer cannot strand an export until next resume.
    private fun drainPendingSaveFlush() {
        // Flush requests before accepting a new ROM launch to preserve
        // save-before-load ordering on the single storage worker.
        var completed = nativePollSaveFlush()
        while (completed != null) {
            try {
                val payload = JSONObject(completed)
                val uri = payload.getString("uri")
                val core = when (payload.getString("core")) {
                    "gpsp" -> "gpSP"
                    else -> "mGBA"
                }
                val target = activeTargets[uri]
                if (target != null && target.core == core) {
                    gameLoader.execute {
                        val stamp = payload.optString("manual_stamp", "")
                        val failures = if (stamp.isNotEmpty()) {
                            RetroArchStorage.exportManualState(this@MainActivity, target, stamp)
                        } else {
                            RetroArchStorage.exportAfterSave(this@MainActivity, target)
                        }
                        if (failures.isNotEmpty()) {
                            runOnUiThread {
                                if (!isDestroyed) {
                                    Log.w(TAG, "Save export warnings: " + failures.joinToString(" | "))
                                    val onlyMirrors = failures.all {
                                        it.startsWith("Shared GBA save sync deferred")
                                    }
                                    val notice = if (onlyMirrors)
                                        "Game saved locally. mGBA/gpSP copy deferred."
                                    else "Game saved locally; some external/state exports need attention."
                                    SlotToast.makeText(this@MainActivity, notice, Toast.LENGTH_LONG).show()
                                }
                            }
                        }
                    }
                }
            } catch (error: Exception) {
                Log.e(TAG, "Invalid save sync notification", error)
            }
            completed = nativePollSaveFlush()
        }
    }

    private val scanSerial = AtomicInteger()
    private var lastStatusPoll: Long = -15000L
    // Fixed native shelf states mirror SHELF_STATUS_TEXT in runtime.rs.
    private val shelfNone = 0
    private val shelfScanning = 1
    private val shelfEmpty = 2
    private val shelfImportError = 3
    private val shelfPermissionExpired = 4
    private val shelfReadError = 5
    private val shelfPickerError = 6
    private val shelfPermissionError = 7

    private lateinit var view: GLSurfaceView
    private var startupSplash: View? = null
    private lateinit var raAchievementOverlay: RaAchievementOverlay

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A staged sign-out must finish before any native HTTP worker is started.
        val cleanSignOut = RaAccount.finishPendingSignOut(filesDir)
        nativeConfigure(filesDir.absolutePath, applicationInfo.nativeLibraryDir)
        // App-private A12 A/B marker; no save or ROM folders are touched.
        val a12Recovery = !File(filesDir, "a12-mgba-legacy-pacing.flag").exists()
        nativeSetMgbaLinkRecoveryEnabled(a12Recovery)
        Log.i(TAG, "A12 recovery mode: " + if (a12Recovery) "a11" else "legacy")
        nativeSetShareSaves(getSharedPreferences(PREFS, MODE_PRIVATE)
            .getBoolean(SHARE_GBA_SAVES, false))
        if (cleanSignOut && RaAccount.configuredUser(filesDir) != null) nativeRaAccountConfigured()
        cartSounds = CartSounds(this)
        view = object : GLSurfaceView(this) {
            init {
                setEGLContextClientVersion(2)
                preserveEGLContextOnPause = true
                setRenderer(object : Renderer {
                    var ready = false
                    private var probeStartNs = 0L
                    private var lastDrawNs = 0L
                    private var callbacks = 0
                    private var gapOver25ms = 0
                    private var gapOver40ms = 0
                    private var gapUsMax = 0L
                    private var nativeUsMax = 0L
                    private var nativeOver16ms = 0
                    private var nativeOver33ms = 0
                    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
                        ready = nativeSurfaceCreated()
                        if (!ready) Log.e(TAG, "Slot GLES initialization failed")
                        else runOnUiThread {
                            // Wait for the renderer to exist before revealing the shelf.
                            uiHandler.postDelayed({ hideSlotStartupSplash() }, 650L)
                        }
                    }
                    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
                        if (ready) nativeSurfaceChanged(width, height)
                    }
                    override fun onDrawFrame(gl: GL10?) {
                        val a12StartedNs = SystemClock.elapsedRealtimeNanos()
                        var a12NativeUs = 0L
                        if (ready) {
                            val beginNativeNs = SystemClock.elapsedRealtimeNanos()
                            nativeDrawFrame()
                            a12NativeUs = (SystemClock.elapsedRealtimeNanos() - beginNativeNs) / 1000L
                            // Notify the existing AudioTrack worker that libretro
                            // may have produced samples. Never block GL on audio.
                            if (audioRunning.get() && nativeIsPlaying() &&
                                audioFrameSignal.availablePermits() == 0
                            ) audioFrameSignal.release()
                            // Drain on the render thread for frame-accurate insert/eject clicks.
                            var kind = nativePollCartSfx()
                            while (kind != 0) {
                                cartSounds.play(kind)
                                kind = nativePollCartSfx()
                            }
                        } else {
                            GLES20.glClearColor(0f, 0f, 0f, 1f)
                            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT)
                        }
                        // Diagnostic-only. EGL swaps after this callback returns:
                        // callback gaps are not true presentation timestamps.
                        if (ready && nativeIsPlaying()) {
                            if (probeStartNs == 0L) probeStartNs = a12StartedNs
                            if (lastDrawNs != 0L) {
                                val gapUs = (a12StartedNs - lastDrawNs) / 1000L
                                gapUsMax = maxOf(gapUsMax, gapUs)
                                if (gapUs > 25_000L) gapOver25ms++
                                if (gapUs > 40_000L) gapOver40ms++
                            }
                            lastDrawNs = a12StartedNs
                            callbacks++
                            nativeUsMax = maxOf(nativeUsMax, a12NativeUs)
                            if (a12NativeUs > 16_667L) nativeOver16ms++
                            if (a12NativeUs > 33_333L) nativeOver33ms++
                            val spanNs = SystemClock.elapsedRealtimeNanos() - probeStartNs
                            if (spanNs >= 5_000_000_000L) {
                                val fpsX10 = callbacks.toLong() * 10_000_000_000L / spanNs
                                Log.i(TAG, "GL callback pacing: callbacks=$callbacks fps_x10=$fpsX10 " +
                                    "gap_over_25ms=$gapOver25ms gap_over_40ms=$gapOver40ms " +
                                    "gap_us_max=$gapUsMax native_us_max=$nativeUsMax " +
                                    "native_over_16ms=$nativeOver16ms native_over_33ms=$nativeOver33ms")
                                probeStartNs = a12StartedNs
                                callbacks = 0
                                gapOver25ms = 0
                                gapOver40ms = 0
                                gapUsMax = 0L
                                nativeUsMax = 0L
                                nativeOver16ms = 0
                                nativeOver33ms = 0
                            }
                        } else {
                            probeStartNs = 0L
                            lastDrawNs = 0L
                        }
                    }
                })
                renderMode = RENDERMODE_CONTINUOUSLY
                isFocusableInTouchMode = true
                requestFocus()
            }


        }
        val frame = FrameLayout(this)
        frame.addView(view, FrameLayout.LayoutParams(-1, -1))
        shelfStatus = SlotStatusIndicators(this, frame)
        taskIndicators = SlotShelfTaskIndicators(this, frame)
        libraryRefreshIndicator = taskIndicators.library
        autoLabelIndicator = taskIndicators.labels
        // There is no Android status view: the native Slot renderer owns all
        // shelf messages and controller hints, even during ROM rescanning.
        performanceHud = TextView(this).apply {
            typeface = SlotUi.font(this@MainActivity)
            textSize = 10.5f
            letterSpacing = .05f
            setTextColor(Color.argb(230, 242, 242, 245))
            // Subtle translucent HUD; keep the game visible behind it.
            setBackgroundColor(Color.argb(166, 23, 22, 25))
            setPadding(10, 5, 10, 5)
            isClickable = false
            isFocusable = false
            visibility = View.GONE
        }
        frame.addView(performanceHud,
            FrameLayout.LayoutParams(-2, -2, Gravity.BOTTOM or Gravity.END).apply {
                rightMargin = (12 * resources.displayMetrics.density).toInt()
                bottomMargin = (12 * resources.displayMetrics.density).toInt()
            })
        raAchievementOverlay = RaAchievementOverlay(this, frame)
        showSlotStartupSplash(frame)
        setContentView(frame)
        immersive()
        uiHandler.post(pulse)
        if (!cleanSignOut) {
            uiHandler.postDelayed({
                SlotToast.makeText(this,
                    "Previous RA sign out needs attention. Local awards were kept.",
                    Toast.LENGTH_LONG).show()
            }, 800L)
        }
        startRaNetworkWatch()
        startRaSmartCache()
        // Stage BIOS files before the first queued ROM launch, using the same
        // worker that loads ROMs; no race between BIOS import and core startup.
        getSharedPreferences(PREFS, MODE_PRIVATE).getString(BIOS_ROOT, null)
            ?.let { savedBios ->
                val uri = Uri.parse(savedBios)
                if (contentResolver.persistedUriPermissions.any { perm ->
                        perm.uri == uri && perm.isReadPermission
                    }) {
                    importBiosFolder(uri, persistSelection = false)
                } else {
                    Log.w(TAG, "BIOS folder access expired; choose it again")
                }
            }
        val saved = getSharedPreferences(PREFS, MODE_PRIVATE).getString(ROM_ROOT, null)
        if (saved != null) {
            val uri = Uri.parse(saved)
            if (contentResolver.persistedUriPermissions.any {
                    it.uri == uri && it.isReadPermission
                }) {
                if (restoreCachedLibrary(uri)) {
                    // Use the same fast cached boot as before; verify the SAF
                    // tree off-thread behind the splash with no UI delay.
                    scanFolder(uri, quiet = true)
                } else scanFolder(uri)
            } else {
                nativeSetShelfStatus(shelfPermissionExpired)
            }
        } else {
            // The native first-run keycap appears on an empty shelf.
            nativeSetShelfStatus(shelfNone)
        }
    }

    /** The unmodified, original Slot v1.5.0 firmware bootlogo.bmp is a 24-bit BMP.
     *  Decode its BGR rows explicitly: Android BitmapFactory does not promise BMP support.
     *  This is an in-app startup screen, never a device/firmware boot partition write.
     */
    private fun readOriginalSlotBootLogo(): Bitmap? = try {
        val bytes = assets.open("slot-bootlogo.bmp").use { it.readBytes() }
        if (bytes.size < 54 || bytes[0] != 'B'.code.toByte() ||
            bytes[1] != 'M'.code.toByte()) {
            null
        } else {
            val data = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            val offset = data.getInt(10)
            val width = data.getInt(18)
            val height = data.getInt(22)
            val planes = data.getShort(26).toInt() and 0xffff
            val bitDepth = data.getShort(28).toInt() and 0xffff
            val compression = data.getInt(30)
            val stride = if (width > 0) ((width.toLong() * 3L + 3L) / 4L) * 4L else 0L
            if (width <= 0 || height <= 0 || width > 2048 || height > 2048 ||
                planes != 1 || bitDepth != 24 || compression != 0 ||
                offset < 54 || offset.toLong() + stride * height > bytes.size) {
                null
            } else {
                val pixels = IntArray(width * height)
                for (y in 0 until height) {
                    val row = offset + ((height - 1 - y) * stride).toInt()
                    for (x in 0 until width) {
                        val at = row + x * 3
                        val b = bytes[at].toInt() and 0xff
                        val g = bytes[at + 1].toInt() and 0xff
                        val r = bytes[at + 2].toInt() and 0xff
                        pixels[y * width + x] = Color.rgb(r, g, b)
                    }
                }
                Bitmap.createBitmap(pixels, width, height, Bitmap.Config.ARGB_8888)
            }
        }
    } catch (error: Exception) {
        Log.w(TAG, "Original Slot boot logo could not be loaded", error)
        null
    }

    private fun showSlotStartupSplash(frame: FrameLayout) {
        // Use the original approved Slot. v1 wordmark as a VectorDrawable.
        // The old 960x640 bitmap was enlarged from a 196x75 firmware logo and
        // looks soft at KONKR resolution. Preserve the original image as a
        // fallback, without touching firmware assets or the emulation UI.
        val vectorLogo = try {
            getDrawable(R.drawable.slot_boot_wordmark_vector)
        } catch (error: Exception) {
            Log.w(TAG, "Vector Slot boot wordmark unavailable", error)
            null
        }
        val fallbackBitmap = if (vectorLogo == null) try {
            assets.open("slot-bootlogo-kpa-960x640.png").use(BitmapFactory::decodeStream)
        } catch (error: Exception) {
            Log.w(TAG, "Original Slot boot logo fallback unavailable", error)
            null
        } else null
        if (vectorLogo == null && fallbackBitmap == null) return

        val overlay = FrameLayout(this).apply {
            setBackgroundColor(Color.rgb(11, 14, 17))
            isFocusable = false
            isClickable = false
        }
        val image = ImageView(this).apply {
            if (vectorLogo != null) setImageDrawable(vectorLogo)
            else setImageBitmap(fallbackBitmap)
            scaleType = ImageView.ScaleType.FIT_CENTER
            contentDescription = "slot. boot logo"
        }
        overlay.addView(image, FrameLayout.LayoutParams(-1, -1))
        frame.addView(overlay, FrameLayout.LayoutParams(-1, -1))
        startupSplash = overlay
    }

    private fun hideSlotStartupSplash() {
        val splash = startupSplash ?: return
        splash.animate().alpha(0f).setDuration(220L).withEndAction {
            (splash.parent as? FrameLayout)?.removeView(splash)
            startupSplash = null
        }.start()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        SlotBuildBehavior.emulatorButton(keyCode)?.let { button ->
            if (event.repeatCount == 0) {
                warmedCandidate.set(null)
                wakeCarouselRender()
                nativeKey(button, true)
            }
            return true
        }
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            onBackPressed()
            return true
        }
        if (keyCode == KeyEvent.KEYCODE_BUTTON_MODE && event.repeatCount == 0) {
            Log.i(TAG, "KONKR MENU BTN_MODE down: Android code=" + keyCode +
                " scan=" + event.scanCode + " device=" + event.deviceId)
        }
        // START / SELECT on the shelf are now rendered by Slot itself.
        // Log any otherwise unrecognized physical key so the KONKR's top-round
        // key can be mapped from real device evidence, not a guessed keycode.
        if (keyCode !in BUTTONS) {
            if (event.repeatCount == 0) {
                Log.i(TAG, "Unmapped hardware key down: code=" + keyCode +
                    " name=" + KeyEvent.keyCodeToString(keyCode) +
                    " device=" + event.deviceId + " scan=" + event.scanCode)
            }
            return super.onKeyDown(keyCode, event)
        }
        if (event.repeatCount == 0) {
            warmedCandidate.set(null)
            wakeCarouselRender()
            nativeKey(keyCode, true)
        }
        return true
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        SlotBuildBehavior.emulatorButton(keyCode)?.let { button ->
            warmedCandidate.set(null)
            wakeCarouselRender()
            nativeKey(button, false)
            return true
        }
        if (keyCode == KeyEvent.KEYCODE_BACK) return true
        if (keyCode == KeyEvent.KEYCODE_BUTTON_MODE) {
            Log.i(TAG, "KONKR MENU BTN_MODE up: Android code=" + keyCode)
        }
        if (keyCode !in BUTTONS) return super.onKeyUp(keyCode, event)
        warmedCandidate.set(null)
        wakeCarouselRender()
        nativeKey(keyCode, false)
        return true
    }

    @Deprecated("Android 12 back key")
    override fun onBackPressed() {
        warmedCandidate.set(null)
        wakeCarouselRender()
        if (nativeIsPlaying()) {
            nativeExitGame()
        } else {
            nativeKey(KeyEvent.KEYCODE_BUTTON_B, true)
            nativeKey(KeyEvent.KEYCODE_BUTTON_B, false)
        }
    }

    private enum class SharedGbaChoice { MGBA, GPSP, CANCEL }

    /** Three-way conflict arbitration. Nothing is written to either
     * cartridge while the dialog is open; Cancel must abort ROM launch.
     */
    private fun askSharedGbaConflict(): SharedGbaChoice {
        val ready = CountDownLatch(1)
        val selected = java.util.concurrent.atomic.AtomicReference(SharedGbaChoice.CANCEL)
        runOnUiThread {
            if (isDestroyed || isFinishing) { ready.countDown(); return@runOnUiThread }
            try {
                val dialog = SlotModalDialog(this, "SAVE CONFLICT",
                    "mGBA and gpSP saves differ. Choose which progress to keep. " +
                        "Both original saves are backed up.",
                    onDismiss = { ready.countDown() })
                // Default to the safe action: accidental A never changes SRAM.
                dialog.addButton("CANCEL") { dialog.dismiss() }
                dialog.addButton("KEEP mGBA SAVE") {
                    selected.set(SharedGbaChoice.MGBA)
                    dialog.dismiss()
                }
                dialog.addButton("KEEP gpSP SAVE") {
                    selected.set(SharedGbaChoice.GPSP)
                    dialog.dismiss()
                }
                dialog.show()
            } catch (error: Exception) {
                Log.e(TAG, "Cannot open shared-save choice", error)
                ready.countDown()
            }
        }
        return if (ready.await(120L, TimeUnit.SECONDS)) selected.get()
            else SharedGbaChoice.CANCEL
    }

    private enum class CoreStateChoice { EXISTING, SHARED, CANCEL }

    /** This dialog is deliberately rare: only Share Saves ON, actual core
     *  change, and an existing destination-core auto-state. The ROM loader
     *  worker waits while the UI thread remains free to handle the answer.
     */
    private fun askCoreSwitchResume(core: String): CoreStateChoice {
        val ready = CountDownLatch(1)
        val selected = java.util.concurrent.atomic.AtomicReference(CoreStateChoice.CANCEL)
        runOnUiThread {
            if (isDestroyed || isFinishing) { ready.countDown(); return@runOnUiThread }
            try {
                val dialog = SlotModalDialog(this, "SAVE STATE",
                    "The $core state may be older than the shared cartridge save. " +
                        "Choose which progress to load.",
                    onDismiss = { ready.countDown() })
                dialog.addButton("CANCEL") { dialog.dismiss() }
                dialog.addButton("SHARED CARTRIDGE SAVE") {
                    selected.set(CoreStateChoice.SHARED)
                    dialog.dismiss()
                }
                dialog.addButton("EXISTING $core STATE") {
                    selected.set(CoreStateChoice.EXISTING)
                    dialog.dismiss()
                }
                dialog.show()
            } catch (error: Exception) {
                Log.e(TAG, "Cannot open core-state choice", error)
                ready.countDown()
            }
        }
        return if (ready.await(120L, TimeUnit.SECONDS)) selected.get()
            else CoreStateChoice.CANCEL
    }

    private fun loadGameFromSaf(rawUri: String) {
        val uri = try { Uri.parse(rawUri) } catch (_: Exception) {
            nativeGameError(rawUri, "Invalid ROM URI")
            return
        }
        val queuedAt = SystemClock.elapsedRealtime()
        gameLoader.execute {
            val launchStart = SystemClock.elapsedRealtime()
            val queueWait = launchStart - queuedAt
            if (queueWait > 20) {
                Log.i(TAG, "Slot launch queue delay: ${queueWait}ms (waiting for preceding save exports)")
            }
            try {
                val name = uri.lastPathSegment.orEmpty().lowercase()
                val ext = when {
                    name.endsWith(".gba") -> "gba"
                    name.endsWith(".gbc") -> "gbc"
                    name.endsWith(".gb") -> "gb"
                    else -> error("Unsupported ROM type")
                }
                val digest = MessageDigest.getInstance("SHA-256")
                    .digest(rawUri.toByteArray(Charsets.UTF_8))
                    .joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }
                val dir = File(cacheDir, "rom-cache").apply { mkdirs() }
                val target = File(dir, "$digest.$ext")
                // Always open via Android SAF; no assumptions about physical filesystem paths.
                if (!target.isFile || target.length() < 0x150) {
                    val temp = File(dir, "$digest.tmp")
                    try {
                        contentResolver.openInputStream(uri)?.use { input ->
                            temp.outputStream().buffered().use { output ->
                                val buffer = ByteArray(64 * 1024)
                                var copied = 0L
                                while (true) {
                                    val n = input.read(buffer)
                                    if (n < 0) break
                                    copied += n
                                    require(copied <= 64L * 1024 * 1024) { "ROM exceeds 64 MiB" }
                                    output.write(buffer, 0, n)
                                }
                                require(copied >= 0x150) { "ROM too small" }
                            }
                        } ?: error("Cannot read this ROM")
                        check(temp.renameTo(target)) { "Cannot cache ROM" }
                    } finally { temp.delete() }
                }
                val romReadyAt = SystemClock.elapsedRealtime()
                val savedCore = nativeCoreForUri(rawUri)
                val core = if (savedCore == "gpsp" && ext == "gba") "gpSP" else "mGBA"
                val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
                val storage = SlotBuildBehavior.storagePolicy(
                    packageName,
                    prefs.getString(SAVE_ROOT, null),
                    prefs.getString(STATE_ROOT, null),
                    prefs.getBoolean(SHARE_GBA_SAVES, false)
                )
                val saves = storage.savesRoot?.let(Uri::parse)
                val states = storage.statesRoot?.let(Uri::parse)
                val shared = RetroArchStorage.target(this, rawUri, core, saves, states,
                    shareGbaSaves = storage.shareGbaSaves)
                try {
                    RetroArchStorage.importBeforeLaunch(this@MainActivity, shared)
                } catch (error: RetroArchStorage.SaveConflict) {
                    Log.w(TAG, "Conflicting mGBA/gpSP SRAM needs explicit choice", error)
                    when (val choice = askSharedGbaConflict()) {
                        SharedGbaChoice.CANCEL -> {
                            nativeGameError(rawUri, "Shared save selection cancelled")
                            return@execute
                        }
                        SharedGbaChoice.MGBA, SharedGbaChoice.GPSP -> {
                            val source = if (choice == SharedGbaChoice.MGBA)
                                "mGBA" else "gpSP"
                            RetroArchStorage.resolveSharedGbaConflict(
                                this@MainActivity, shared, source)
                            RetroArchStorage.importBeforeLaunch(this@MainActivity, shared)
                        }
                    }
                }
                // Share Saves never silently chooses an old same-core state
                // over freshly imported cross-core SRAM. A direct same-core
                // launch has no dialogue and behaves exactly as before.
                val previous = if (ext == "gba")
                    prefs.getString("last_gba_core_$digest", null) else null
                val switched = ext == "gba" && shared.shareGbaSaves &&
                    previous != null && previous != core
                var resumeChoice = -1 // Existing default Slot behavior.
                if (switched) {
                    val exists = RetroArchStorage.hasCoreAutoState(this@MainActivity, shared)
                    if (RetroArchStorage.shouldOfferCoreResume(
                            shared.shareGbaSaves, previous, core, exists)) {
                        when (askCoreSwitchResume(core)) {
                            CoreStateChoice.CANCEL -> {
                                nativeGameError(rawUri, "Core switch cancelled")
                                return@execute
                            }
                            CoreStateChoice.EXISTING -> {
                                // Do not delete the one-shot SRAM guard until
                                // an explicitly requested external RetroArch
                                // state has actually been decoded and staged.
                                // Existing protected generations remain higher
                                // priority than stale external auto-states.
                                RetroArchStorage.importExplicitExistingState(
                                    this@MainActivity, shared)
                                val lower = if (core == "mGBA") "mgba" else "gpsp"
                                // The previous SRAM-first attempt may have
                                // allocated a generation but never written a
                                // state (e.g. Android killed the app before
                                // Save & Eject). Explicitly choosing the OLD
                                // state must not be masked by that empty
                                // redirect. Completed generations stay valid.
                                val genIndex = File(filesDir,
                                    "Saves/$digest.$lower.state-generation.active")
                                if (genIndex.isFile) {
                                    val name = genIndex.readText()
                                    val valid = name.startsWith("$digest.$lower.gen-") &&
                                        name.matches(Regex("[a-zA-Z0-9.-]{1,180}"))
                                    val genState = File(filesDir, "Saves/$name.state")
                                    if (!valid) error("Invalid protected state generation")
                                    if (!genState.isFile) {
                                        if (!genIndex.delete()) {
                                            error("Cannot select existing state over empty generation")
                                        }
                                        val hold = File(filesDir,
                                            "Saves/$digest.$lower.state-export-protected")
                                        if (hold.isFile && !hold.delete()) {
                                            error("Cannot clear empty generation export guard")
                                        }
                                    }
                                }
                                val skipped = File(filesDir,
                                    "Saves/$digest.$lower.skip-resume.once")
                                if (skipped.isFile && !skipped.delete()) {
                                    error("Cannot restore existing state while SRAM guard is active")
                                }
                                resumeChoice = 1
                            }
                            CoreStateChoice.SHARED -> {
                                val lower = if (core == "mGBA") "mgba" else "gpsp"
                                RetroArchStorage.stage(File(filesDir,
                                    "Saves/$digest.$lower.skip-resume.once"), byteArrayOf(1))
                                resumeChoice = 0
                            }
                        }
                    } else {
                        // No old state on the new core: start from shared
                        // SRAM, not a state from the previous emulator.
                        resumeChoice = 0
                    }
                }
                activeTargets[rawUri] = shared
                val readyAt = SystemClock.elapsedRealtime()
                Log.i(TAG, "Slot launch preparation: ROM cache=${romReadyAt - launchStart}ms, " +
                    "RetroArch import=${readyAt - romReadyAt}ms, total=${readyAt - launchStart}ms")
                nativeSetNextResumeMode(resumeChoice)
                nativeGameReady(rawUri, target.absolutePath)
                if (ext == "gba") prefs.edit().putString("last_gba_core_$digest", core).apply()
            } catch (error: Exception) {
                Log.e(TAG, "Could not prepare ROM", error)
                nativeGameError(rawUri, error.message ?: "Cannot open ROM")
            }
        }
    }

    private fun pollRumble() {
        if (!resumed || !nativeIsPlaying()) return
        val strength = nativePollRumbleStrength()
        if (strength <= 4096) return
        val now = SystemClock.elapsedRealtime()
        if (now - lastRumbleAt < 100L) return
        lastRumbleAt = now
        val device = vibrator ?: return
        if (!device.hasVibrator()) return
        try {
            val intensity = (strength / 257).coerceIn(10, 255)
            device.vibrate(VibrationEffect.createOneShot(45L, intensity))
        } catch (error: RuntimeException) {
            Log.w(TAG, "KONKR rumble unavailable", error)
        }
    }

    private fun startAudio() {
        if (!audioRunning.compareAndSet(false, true)) return
        audioFrameSignal.drainPermits()
        audioThread = Thread({
            // Android's streaming AudioTrack is fed from this app-owned
            // worker. Prefer the platform's documented audio scheduling
            // priority so transient GL/Wi-Fi activity cannot preempt the
            // PCM writer unnecessarily. Do NOT change the playback buffer
            // or add latency to hide actual missing linked game frames.
            try {
                android.os.Process.setThreadPriority(
                    android.os.Process.THREAD_PRIORITY_AUDIO)
                Log.i(TAG, "Audio worker: THREAD_PRIORITY_AUDIO enabled")
            } catch (error: Exception) {
                Log.w(TAG, "Audio priority unavailable; using default worker priority", error)
            }
            var player: AudioTrack? = null
            var rate = 0
            var requestedSpeed = 0
            var observedUnderruns = 0
            var audioProbeAt = 0L
            var emptyAudioReads = 0
            var playedShorts = 0L
            var audioBufferBytes = 0
            try {
                while (audioRunning.get()) {
                    val wanted = nativeAudioSampleRate()
                    if (wanted <= 0) {
                        player?.pause()
                        SystemClock.sleep(35)
                        continue
                    }
                    if (player == null || rate != wanted) {
                        player?.release()
                        val minBytes = AudioTrack.getMinBufferSize(
                            wanted, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT
                        )
                        if (minBytes <= 0) {
                            SystemClock.sleep(100)
                            continue
                        }
                        // Preserve the validated low-latency legacy AudioTrack buffer.
                        val bufferBytes = maxOf(16384, minBytes * 2)
                        audioBufferBytes = bufferBytes
                        Log.i(TAG, "Audio diagnostic: rate=$wanted Hz, " +
                            "minBuffer=$minBytes bytes, allocated=$bufferBytes bytes")
                        player = AudioTrack.Builder()
                            .setAudioAttributes(AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_GAME)
                                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
                            .setAudioFormat(AudioFormat.Builder()
                                .setSampleRate(wanted)
                                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                                .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build())
                            .setBufferSizeInBytes(bufferBytes)
                            .setTransferMode(AudioTrack.MODE_STREAM)
                            .build()
                        rate = wanted
                        requestedSpeed = 0
                        observedUnderruns = 0
                        emptyAudioReads = 0
                        playedShorts = 0L
                        audioProbeAt = SystemClock.elapsedRealtime()
                        player.play()
                    } else if (player.playState != AudioTrack.PLAYSTATE_PLAYING) {
                        // A paused legacy buffer is reused when the next game has
                        // the same sample rate; it still must be resumed.
                        player.play()
                    }
                    val speed = nativeAudioSpeedPermille().coerceIn(1000, 6000)
                    if (speed != requestedSpeed) {
                        requestedSpeed = speed
                        try {
                            // Android time-stretches at the selected speed while preserving
                            // the ORIGINAL sample pitch (not chipmunk audio).
                            // If the Audio HAL cannot stretch, mute rather
                            // than silently playing an incorrect-pitch stream.
                            val params = PlaybackParams()
                                .setSpeed(speed / 1000f)
                                .setPitch(1.0f)
                                .setAudioFallbackMode(PlaybackParams.AUDIO_FALLBACK_MODE_MUTE)
                            player.playbackParams = params
                            nativeFastAudioSupported(true)
                            Log.i(TAG, "FF audio: speed=${speed / 1000f}x, pitch=1.0x")
                        } catch (error: Exception) {
                            Log.w(TAG, "Pitch-preserving FF unsupported; silent fallback", error)
                            nativeFastAudioSupported(false)
                            try {
                                player.playbackParams = PlaybackParams()
                                    .setSpeed(1.0f).setPitch(1.0f)
                            } catch (_: Exception) { }
                        }
                    }
                    val samples = nativeReadAudio()
                    if (samples.isEmpty()) {
                        emptyAudioReads++
                        // Event-driven wakeup instead of sleeping blindly for 8ms:
                        // GLSurfaceView signals after each libretro callback.
                        // The 30ms timeout is a safety fallback for paused frames.
                        try {
                            audioFrameSignal.tryAcquire(30, TimeUnit.MILLISECONDS)
                        } catch (_: InterruptedException) {
                            Thread.currentThread().interrupt()
                            break
                        }
                        audioFrameSignal.drainPermits()
                        continue
                    }
                    audioFrameSignal.drainPermits()
                    val written = player.write(samples, 0, samples.size, AudioTrack.WRITE_BLOCKING)
                    if (written < 0) {
                        Log.w(TAG, "AudioTrack write error: $written")
                    } else {
                        playedShorts += written.toLong()
                    }
                    // Diagnostic only; do not modify audio queue, buffer size or
                    // event-driven wakeup. A rise here distinguishes Android
                    // playback underruns from mGBA mixer discontinuities.
                    val probeNow = SystemClock.elapsedRealtime()
                    if (probeNow - audioProbeAt >= 5_000L) {
                        val underflows = player.underrunCount
                        if (underflows > observedUnderruns) {
                            Log.i(TAG, "AudioTrack underruns: +${underflows - observedUnderruns}" +
                                " over 5s (total=$underflows, rate=$wanted Hz)")
                        }
                        if (underflows > observedUnderruns) {
                            Log.i(TAG, "Audio diagnostic: rate=$wanted, " +
                                "bufferBytes=$audioBufferBytes, " +
                                "writtenShorts=$playedShorts over 5s, " +
                                "emptyReads=$emptyAudioReads, " +
                                "underrunDelta=${underflows - observedUnderruns}")
                        }
                        val spanMs = (probeNow - audioProbeAt).coerceAtLeast(1L)
                        val expected = wanted.toLong() * 2L * spanMs / 1000L
                        // Report even when there are no underruns, to isolate
                        // PCM producer starvation from AudioTrack HAL behaviour.
                        Log.i(TAG, "A12 PCM pacing: rate=$wanted span_ms=$spanMs " +
                            "written_shorts=$playedShorts expected_shorts=$expected " +
                            "empty_reads=$emptyAudioReads queued_shorts=${nativeAudioQueuedShorts()} " +
                            "underruns=${underflows - observedUnderruns}")
                        observedUnderruns = underflows
                        playedShorts = 0L
                        emptyAudioReads = 0
                        audioProbeAt = probeNow
                    }
                }
            } catch (error: Exception) {
                Log.e(TAG, "Android AudioTrack error", error)
            } finally {
                player?.stop()
                player?.release()
            }
        }, "SlotKONKR-Audio").apply { isDaemon = true; start() }
    }

    private fun stopAudio() {
        audioRunning.set(false)
        // Unblock the audio worker if it is waiting on a video frame.
        audioFrameSignal.release()
        audioThread?.interrupt()
        audioThread = null
    }

    /**
     * Read SAF ROM fingerprints exclusively off UI/audio/GL threads. Only
     * the complete ROM MD5 is forwarded to the native RA preparation queue.
     */
    private fun startRaSmartCache() {
        if (isDestroyed || RaAccount.configuredUser(filesDir) == null) return
        if (!raSmartRunning.compareAndSet(false, true)) return
        try {
            raSmartWorker.execute {
                try {
                    val manager = getSystemService(CONNECTIVITY_SERVICE) as? ConnectivityManager
                    val caps = manager?.getNetworkCapabilities(manager.activeNetwork)
                    if (caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) != true)
                        return@execute
                    RaSmartCache.indexAndPrefetch(applicationContext, filesDir,
                        isPlaying = { nativeIsPlaying() },
                        awaitForeground = {
                            // While Android is asleep, keep already submitted HTTP
                            // cache downloads going, but park the SAF indexer.
                            synchronized(raSmartForeground) {
                                while (!resumed && !Thread.currentThread().isInterrupted) {
                                    raSmartForeground.wait()
                                }
                            }
                            if (Thread.currentThread().isInterrupted)
                                throw InterruptedException()
                        },
                        submit = { hash, console -> nativeRaPrefetchHash(hash, console) })
                } catch (_: Exception) {
                    Log.w(TAG, "RA Smart Cache indexing postponed")
                } finally {
                    raSmartRunning.set(false)
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            raSmartRunning.set(false)
        }
    }

    private fun startAutoCartLabels() {
        if (!resumed || isDestroyed || nativeIsPlaying() || cartStudioOpening) return
        if (!autoLabelsRunning.compareAndSet(false, true)) {
            // A short Studio visit may return before the previous worker exits.
            autoLabelsRestartRequested.set(true)
            if (!autoLabelsRunning.compareAndSet(false, true)) return
        }
        autoLabelsRestartRequested.set(false)
        try {
            autoLabelsWorker.execute {
                var anotherBatch = false
                AutoCartLabelDiagnostics.clear()
                try {
                    val manager = getSystemService(CONNECTIVITY_SERVICE) as? ConnectivityManager
                    val caps = manager?.getNetworkCapabilities(manager.activeNetwork)
                    if (caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) != true) {
                        Log.i("SlotAutoLabels", "Deferred: Android default Internet network is not validated")
                        return@execute
                    }
                    // Exactly the production 1.2.0 callback: enqueue a
                    // successful saved label for an idle carousel repaint.
                    // Status/UI snapshots are completely separate.
                    anotherBatch = AutoCartLabels.fill(applicationContext,
                        shouldContinue = { resumed && !isDestroyed && !nativeIsPlaying() && !cartStudioOpening },
                        onSaved = { uri -> autoLabelUpdates.add(uri) })
                } catch (error: Exception) {
                    Log.w("SlotAutoLabels", "Automatic artwork deferred", error)
                } finally {
                    autoLabelsRunning.set(false)
                    val recheck = autoLabelsRestartRequested.getAndSet(false)
                    val report = AutoCartLabelDiagnostics.snapshot()
                    if (!anotherBatch && !recheck && report.total > 0) {
                        Log.i("SlotAutoLabels",
                            "ART_SUMMARY examined=${report.checked}/${report.total} " +
                                "saved=${report.saved} not_found=${report.notFound} " +
                                "network_error=${report.networkError} " +
                                "invalid_artwork=${report.invalidArtwork} " +
                                "other_error=${report.otherError}")
                        uiHandler.post {
                            if (::autoLabelIndicator.isInitialized) {
                                if (resumed && !isDestroyed && !nativeIsPlaying() &&
                                    !cartStudioOpening) autoLabelIndicator.finished(report.saved)
                                else autoLabelIndicator.hide()
                            }
                        }
                    } else if (!anotherBatch && !recheck && report.total == 0) {
                        // No indexed ROMs or network yet: never leave an
                        // old spinner visible while the worker is stopped.
                        uiHandler.post {
                            if (::autoLabelIndicator.isInitialized) autoLabelIndicator.hide()
                        }
                    }
                    if ((anotherBatch || recheck) && resumed && !isDestroyed &&
                        !nativeIsPlaying() && !cartStudioOpening) {
                        // A pause between bounded batches prevents continuous
                        // ROM reads and leaves the carousel's render thread free.
                        uiHandler.postDelayed({ startAutoCartLabels() }, 4_000L)
                    }
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            autoLabelsRunning.set(false)
        }
    }

    /** Android-confirmed internet access, not Wi-Fi enabled or cached RA metadata.
     * Snapshotted when the game-ready notice is queued; no HTTP request on UI.
     */
    private fun hasValidatedInternet(): Boolean = try {
        val manager = raConnectivity
            ?: (getSystemService(CONNECTIVITY_SERVICE) as? ConnectivityManager)
        val capabilities = manager?.getNetworkCapabilities(manager.activeNetwork)
        capabilities?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true &&
            capabilities?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true
    } catch (_: RuntimeException) {
        false
    }

    private fun startRaNetworkWatch() {
        try {
            val manager = getSystemService(CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return
            manager.registerDefaultNetworkCallback(raConnectivityCallback)
            raConnectivity = manager
        } catch (error: Exception) {
            // No background polling fallback: the HTTP engine already retries
            // with bounded exponential backoff when notifications are unavailable.
            Log.w(TAG, "RA network reconnect callback unavailable")
        }
    }

    /** Snapshot provided by the native renderer, never a network operation. */
    private fun retroAchievementsStatus(): String {
        val raw = nativeRaStatus() ?: return "Preparing"
        return try {
            when (JSONObject(raw).optString("state", "Disabled")) {
                "Ready" -> "Online"
                "Syncing" -> "Preparing"
                "Offline" -> "Offline"
                "Attention" -> "Needs attention"
                else -> "Inactive"
            }
        } catch (_: Exception) { "Unknown" }
    }

    private fun refreshRaPanel(panel: RaPanelDialog, username: String) {
        scanner.execute {
            val hasPrevious = RaDiagnostics.lastSnapshot(username) != null
            val snapshot = RaDiagnostics.inspect(
                filesDir, username,
                onRulesReady = if (hasPrevious) null else { partial ->
                    runOnUiThread {
                        if (!isDestroyed) panel.update(retroAchievementsStatus(), hasValidatedInternet(), partial)
                    }
                }
            )
            runOnUiThread {
                if (!isDestroyed) panel.update(retroAchievementsStatus(), hasValidatedInternet(), snapshot)
            }
        }
    }

    /**
     * Confirm flush of earned awards, then await BOTH account-scoped native
     * workers before revoking credentials. Keep Slot running throughout.
     */
    private fun confirmRaSignOut(panel: RaPanelDialog, username: String) {
        if (nativeIsPlaying()) {
            SlotToast.makeText(this, "Eject your game before switching RA accounts.",
                Toast.LENGTH_LONG).show()
            return
        }
        scanner.execute {
            val pending = RaDiagnostics.pendingForSignOut(filesDir, username)
            runOnUiThread {
                if (isDestroyed) return@runOnUiThread
                val message = when {
                    pending == null ->
                        "Pending awards couldn't be checked. They stay on this device. " +
                            "Reconnect as $username to try syncing them."
                    pending > 0 ->
                        "$pending awards await upload for $username. " +
                            "They stay on this device. Sign in again to sync them; " +
                            "delayed uploads may expire."
                    else ->
                        "Sign out of $username? Local awards and cached games stay."
                }
                val confirm = SlotModalDialog(
                    this,
                    "SIGN OUT",
                    "$message\n\nSlot will keep running after sign-out."
                )
                confirm.addButton("STAY SIGNED IN") { confirm.dismiss() }
                confirm.addButton("SIGN OUT") {
                    val unsaved = runCatching {
                        JSONObject(nativeRaStatus() ?: "{}").optBoolean("unsaved", false)
                    }.getOrDefault(false)
                    if (unsaved) {
                        confirm.setError("Awards still need to be written locally. " +
                            "Sign out is blocked to protect your progress.")
                    } else {
                        confirm.dismiss()
                        flushRaBeforeSignOut(panel, username, 0)
                    }
                }
                confirm.show()
            }
        }
    }

    /** Try up to ~2.5s without blocking the UI or the game/emulator threads. */
    private fun flushRaBeforeSignOut(panel: RaPanelDialog, username: String, attempt: Int) {
        if (isDestroyed || nativeIsPlaying()) return
        if (attempt >= 25) {
            SlotToast.makeText(this,
                "Achievement writes haven't finished. Sign out cancelled to keep your awards.",
                Toast.LENGTH_LONG).show()
            return
        }
        view.queueEvent {
            val ready = nativeRaFlushReady()
            runOnUiThread {
                if (isDestroyed) return@runOnUiThread
                if (ready) {
                    stopRaBeforeSignOut(panel, username, 0)
                } else {
                    uiHandler.postDelayed({
                        flushRaBeforeSignOut(panel, username, attempt + 1)
                    }, 100L)
                }
            }
        }
    }

    /**
     * Native stop is asynchronous: wait for the evaluator AND for any HTTPS
     * request already in progress, before deleting the old session token.
     * On failure/timeout, leave the account configured and its awards intact.
     */
    private fun stopRaBeforeSignOut(panel: RaPanelDialog, username: String, attempt: Int) {
        if (isDestroyed || nativeIsPlaying()) return
        if (attempt >= 150) {
            SlotToast.makeText(this,
                "Sign out couldn't safely stop RetroAchievements. Restart Slot before retrying.",
                Toast.LENGTH_LONG).show()
            return
        }
        view.queueEvent {
            val ready = nativeRaStopReady()
            runOnUiThread {
                if (isDestroyed) return@runOnUiThread
                if (!ready) {
                    uiHandler.postDelayed({
                        stopRaBeforeSignOut(panel, username, attempt + 1)
                    }, 100L)
                    return@runOnUiThread
                }
                // The old-account workers have acknowledged termination.
                // Retain cached awards and game data for later sync.
                val success = RaAccount.beginSignOut(filesDir, username)
                    && RaAccount.finishPendingSignOut(filesDir)
                if (success) {
                    panel.dismiss()
                    SlotToast.makeText(this,
                        "RetroAchievements signed out. Local awards were kept.",
                        Toast.LENGTH_LONG).show()
                    openRetroAchievementsAccount()
                } else {
                    SlotToast.makeText(this,
                        "Sign out needs attention. Your local awards were kept. Restart Slot to retry.",
                        Toast.LENGTH_LONG).show()
                }
            }
        }
    }

    /** The account panel uses the exact Slot label font instead of Android's AlertDialog. */
    private fun openRetroAchievementsAccount() {
        val existing = RaAccount.configuredUser(filesDir)
        if (existing != null) {
            lateinit var panel: RaPanelDialog
            panel = RaPanelDialog(
                activity = this, username = existing,
                onRefresh = { refreshRaPanel(panel, existing) },
                onSignOut = { confirmRaSignOut(panel, existing) },
                onDismiss = {
                    activeRaPanel = null
                    nativeExternalReturn()
                }
            )
            activeRaPanel = panel to existing
            panel.show()
            // Show last known values immediately when reopening the dashboard.
            // They are refreshed safely in the background on every opening.
            RaDiagnostics.lastSnapshot(existing)?.let {
                panel.update(retroAchievementsStatus(), hasValidatedInternet(), it)
            }
            refreshRaPanel(panel, existing)
            // Badge downloads continue in the native HTTP worker; update this
            // read-only dashboard automatically without requiring a new tap.
            val liveUpdates = object : Runnable {
                override fun run() {
                    if (isDestroyed || !panel.isShowing()) return
                    refreshRaPanel(panel, existing)
                    uiHandler.postDelayed(this, 15_000L)
                }
            }
            uiHandler.postDelayed(liveUpdates, 15_000L)
            return
        }

        // A successful login transitions directly to the RA dashboard.
        var openingDashboard = false
        val login = SlotModalDialog(
            this,
            "RETROACHIEVEMENTS",
            "Sign in securely. Achievements use Casual mode.",
            onDismiss = { if (!openingDashboard) nativeExternalReturn() }
        )
        val username = login.addInput("USERNAME", "RetroAchievements username")
        val password = login.addInput("PASSWORD", "Password (not saved)", password = true)
        // Primary action first, then Cancel, in both visual and D-pad order.
        val signIn = login.addButton("SIGN IN") {
            val name = username.text.toString().trim()
            val secret = password.text.toString()
            if (name.isEmpty() || secret.isEmpty()) {
                login.setError("Enter both your RetroAchievements username and password.")
            } else {
                password.text.clear() // Don't leave credentials in the visible form.
                login.setError("Connecting securely…")
                login.setBusy(true)
                raLoginWorker.execute {
                    val result = RaAccount.authenticateAndSave(filesDir, name, secret)
                    runOnUiThread {
                        if (isDestroyed) return@runOnUiThread
                        when (result) {
                            is RaAccount.Result.Connected -> {
                                openingDashboard = true
                                login.dismiss()
                                nativeRaAccountConfigured()
                                startRaSmartCache()
                                openRetroAchievementsAccount()
                                SlotToast.makeText(this,
                                    "RA connected as " + result.username +
                                        ". Preparing GB/GBC/GBA achievements in background.",
                                    Toast.LENGTH_LONG).show()
                            }
                            is RaAccount.Result.Failed -> {
                                login.setBusy(false)
                                login.setError(result.message)
                            }
                        }
                    }
                }
            }
        }
        login.addButton("CANCEL") { login.dismiss() }
        password.setOnEditorActionListener { _, actionId, _ ->
            if (actionId == android.view.inputmethod.EditorInfo.IME_ACTION_DONE) {
                signIn.performClick()
                true
            } else false
        }
        login.show()
    }

    private fun pollSystemStatus() {
        val now = SystemClock.elapsedRealtime()
        if (now - lastStatusPoll < 15000L) return
        lastStatusPoll = now
        // PR #2 by Its-TroyM: JNI passes locale-independent 24h ASCII;
        // Slot's native saved preference handles 12h AM/PM presentation.
        val clock = java.text.SimpleDateFormat("HH:mm", java.util.Locale.ROOT)
            .format(java.util.Date())
        val battery = registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
        val percent = if (level >= 0 && scale > 0) (100 * level / scale).coerceIn(0, 100)
            else -1
        val state = battery?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
        val charging = state == BatteryManager.BATTERY_STATUS_CHARGING ||
            state == BatteryManager.BATTERY_STATUS_FULL
        nativeSystemStatus(clock, percent, charging)

    }

    /** OFF is the safe default, including upgrades from pre-option builds.
     *  Never merge mGBA/gpSP state files; enabling opt-in affects only the
     *  next ROM launch and refuses genuine two-core SRAM conflicts.
     */
    private fun toggleShareGbaSaves() {
        if (nativeIsPlaying()) {
            SlotToast.makeText(this, "Eject the cartridge before changing Save sharing.",
                Toast.LENGTH_LONG).show()
            return
        }
        val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
        val enabled = !prefs.getBoolean(SHARE_GBA_SAVES, false)
        if (!prefs.edit().putBoolean(SHARE_GBA_SAVES, enabled).commit()) {
            SlotToast.makeText(this, "Unable to save Share Saves preference.",
                Toast.LENGTH_LONG).show()
            return
        }
        nativeSetShareSaves(enabled)
        SlotToast.makeText(this,
            if (enabled) "Share Saves ON: mGBA/gpSP SRAM only; conflicts are never overwritten."
            else "Share Saves OFF: mGBA and gpSP saves kept separate.",
            Toast.LENGTH_LONG).show()
    }

    private fun refreshLibrary() {
        val saved = getSharedPreferences(PREFS, MODE_PRIVATE).getString(ROM_ROOT, null)
        if (saved == null) {
            SlotToast.makeText(this, "Choose your games folder first.", Toast.LENGTH_SHORT).show()
            openRomFolderPicker()
            return
        }
        val uri = Uri.parse(saved)
        if (contentResolver.persistedUriPermissions.none { it.uri == uri && it.isReadPermission }) {
            SlotToast.makeText(this, "Games folder access expired. Choose it again.", Toast.LENGTH_SHORT).show()
            openRomFolderPicker()
            return
        }
        scanFolder(uri)
    }

    private fun openSharedFolderPicker(requestCode: Int) {
        check(requestCode == SAVE_REQUEST || requestCode == STATE_REQUEST)
        SlotBuildBehavior.externalFolderRestrictionMessage(packageName)?.let { restriction ->
            SlotToast.makeText(this, restriction, Toast.LENGTH_LONG).show()
            return
        }
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, requestCode)
        } catch (error: ActivityNotFoundException) {
            SlotToast.makeText(this, "Android folder picker unavailable", Toast.LENGTH_LONG).show()
        }
    }

    private fun openBiosFolderPicker() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, BIOS_REQUEST)
        } catch (error: ActivityNotFoundException) {
            SlotToast.makeText(this, "Can't open the BIOS folder picker.", Toast.LENGTH_LONG).show()
        }
    }

    /** Runs sequentially with ROM materialization, never on the UI/GL thread. */
    private fun importBiosFolder(uri: Uri, persistSelection: Boolean) {
        gameLoader.execute {
            try {
                val imported = BiosLibrary.importFrom(this, uri)
                // Restoring the already selected BIOS at startup is silent.
                // Only the first explicit selection ever shows 'BIOS ready'.
                val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
                val firstExplicitImport = persistSelection &&
                    !prefs.contains(BIOS_ROOT)
                if (persistSelection) {
                    prefs.edit().putString(BIOS_ROOT, uri.toString()).apply()
                }
                if (firstExplicitImport) runOnUiThread {
                    if (!isDestroyed) {
                        SlotToast.makeText(
                            this,
                            "BIOS imported: " + imported.joinToString(", ") +
                                " (available next time you start a game)",
                            Toast.LENGTH_LONG
                        ).show()
                    }
                }
            } catch (error: Exception) {
                Log.e(TAG, "BIOS folder import failed", error)
                runOnUiThread {
                    if (!isDestroyed) {
                        SlotToast.makeText(this, "Couldn't import BIOS files: " +
                            (error.message ?: "check folder files"), Toast.LENGTH_LONG).show()
                    }
                }
            }
        }
    }

    private fun openRomFolderPicker() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, FOLDER_REQUEST)
        } catch (error: ActivityNotFoundException) {
            nativeSetShelfStatus(shelfPickerError)
            Log.w(TAG, "Android folder picker unavailable", error)
        }
    }

    private fun openVisualPicker(kind: Int) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = if (kind == THEME_REQUEST) "text/plain" else "image/png"
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, kind)
        } catch (error: ActivityNotFoundException) {
            SlotToast.makeText(this, "Can't open the file picker on this device.", Toast.LENGTH_LONG).show()
        }
    }

    private fun visualFile(kind: Int): File {
        return if (kind == THEME_REQUEST) File(filesDir, "Config/theme.txt")
               else File(filesDir, "Wallpapers/user.png")
    }

    private fun resetVisualFile(kind: Int) {
        val file = visualFile(kind)
        if (file.exists() && !file.delete()) {
            SlotToast.makeText(this, "Couldn't remove the wallpaper. Please try again.", Toast.LENGTH_LONG).show()
            return
        }
        nativeReloadVisualAssets()
    }

    private fun importVisualFile(kind: Int, uri: Uri) {
        try {
            val maxBytes = if (kind == THEME_REQUEST) 64 * 1024 else 6 * 1024 * 1024
            val bytes = contentResolver.openInputStream(uri)?.use { stream ->
                val output = ByteArrayOutputStream()
                val chunk = ByteArray(8192)
                while (true) {
                    val count = stream.read(chunk)
                    if (count < 0) break
                    if (output.size() + count > maxBytes) {
                        throw IllegalArgumentException("Selected file is too large")
                    }
                    output.write(chunk, 0, count)
                }
                output.toByteArray()
            } ?: throw IllegalArgumentException("Selected file could not be opened")
            if (kind == WALLPAPER_REQUEST) {
                val png = byteArrayOf(-119, 80, 78, 71, 13, 10, 26, 10)
                require(bytes.size >= 8 && bytes.copyOfRange(0, 8).contentEquals(png)) {
                    "Wallpaper must be a PNG"
                }
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                require(bounds.outWidth in 1..4096 && bounds.outHeight in 1..4096) {
                    "Unsupported wallpaper dimensions"
                }
            }
            val target = visualFile(kind)
            target.parentFile?.mkdirs()
            val atomic = AtomicFile(target)
            val output = atomic.startWrite()
            try {
                output.write(bytes)
                atomic.finishWrite(output)
            } catch (error: Exception) {
                atomic.failWrite(output)
                throw error
            }
            nativeReloadVisualAssets()
            SlotToast.makeText(this, "Wallpaper updated.", Toast.LENGTH_SHORT).show()
        } catch (error: Exception) {
            Log.e(TAG, "Cannot import Slot visual file", error)
            SlotToast.makeText(this, "Couldn't import this file: " + error.message, Toast.LENGTH_LONG).show()
        }
    }

    private fun cartLabelFile(romUri: String): File =
        CartLabelPaths.file(filesDir, romUri)

    private fun openCartStudio(selected: String?) {
        cartStudioOpening = true
        pendingCartStudioUri = selected
        val intent = Intent(this, CartStudioActivity::class.java).apply {
            if (selected != null) putExtra(CartStudioActivity.EXTRA_ROM, selected)
        }
        @Suppress("DEPRECATION")
        try {
            returnFromSlotChild = true
            startActivityForResult(intent, CART_STUDIO_REQUEST)
            SlotTransitions.open(this)
        } catch (error: Exception) {
            cartStudioOpening = false
            returnFromSlotChild = false
            nativeExternalReturn()
            Log.w(TAG, "Cannot open Cart Studio", error)
            SlotToast.makeText(this, "Unable to open Cart Studio.",
                Toast.LENGTH_SHORT).show()
        }
    }

    private fun openAllCartStudio() = openCartStudio(null)

    private fun openSelectedCartStudio() {
        val selected = nativePollCartLabelUri()
        if (selected.isNullOrBlank()) {
            SlotToast.makeText(this, "Select a game first.", Toast.LENGTH_SHORT).show()
            return
        }
        openCartStudio(selected)
    }

    private fun setStudioOverrides(json: String): Int =
        nativeSetLibrary(CartStudioCatalog.withOverrides(this, json))

    private fun openCartLabelPicker() {
        val selected = nativePollCartLabelUri()
        if (selected.isNullOrBlank()) {
            SlotToast.makeText(this, "Select a game first.", Toast.LENGTH_SHORT).show()
            return
        }
        pendingCartLabelUri = selected
        openVisualPicker(LABEL_REQUEST)
    }

    private fun removeSelectedCartLabel() {
        val selected = nativePollCartLabelUri() ?: return
        val file = cartLabelFile(selected)
        if (file.exists() && !file.delete()) {
            SlotToast.makeText(this, "Couldn't remove the selected artwork.", Toast.LENGTH_LONG).show()
            return
        }
        nativeReloadCartLabel(selected)
        SlotToast.makeText(this, "Custom artwork removed. Original cart label restored.", Toast.LENGTH_SHORT).show()
    }

    private fun importCartLabel(source: Uri, romUri: String) {
        try {
            // Only standalone PNG artwork: not a ROM/BIOS folder or a
            // serialized game state. Decode bounds before storing it.
            val bytes = contentResolver.openInputStream(source)?.use { stream ->
                val output = ByteArrayOutputStream()
                val chunk = ByteArray(8192)
                while (true) {
                    val count = stream.read(chunk)
                    if (count < 0) break
                    if (output.size() + count > 2 * 1024 * 1024) {
                        throw IllegalArgumentException("Cartridge label exceeds 2 MiB")
                    }
                    output.write(chunk, 0, count)
                }
                output.toByteArray()
            } ?: throw IllegalArgumentException("Cannot open label image")
            val signature = byteArrayOf(-119, 80, 78, 71, 13, 10, 26, 10)
            require(bytes.size >= 8 && bytes.copyOfRange(0, 8).contentEquals(signature)) {
                "Cartridge artwork must be a PNG"
            }
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            require(bounds.outWidth in 1..2048 && bounds.outHeight in 1..2048) {
                "Unsupported cartridge-label dimensions"
            }
            val target = cartLabelFile(romUri)
            target.parentFile?.mkdirs()
            val atomic = AtomicFile(target)
            val output = atomic.startWrite()
            try {
                output.write(bytes)
                atomic.finishWrite(output)
            } catch (error: Exception) {
                atomic.failWrite(output)
                throw error
            }
            nativeReloadCartLabel(romUri)
            SlotToast.makeText(this, "Cart artwork updated.", Toast.LENGTH_SHORT).show()
        } catch (error: Exception) {
            Log.e(TAG, "Could not import cartridge label", error)
            SlotToast.makeText(this, "Couldn't import cart artwork: " + error.message, Toast.LENGTH_LONG).show()
        }
    }

    @Deprecated("Android 12 SAF activity result")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == CART_STUDIO_REQUEST) {
            val selected = pendingCartStudioUri
            pendingCartStudioUri = null
            if (resultCode == RESULT_OK) {
                // Rebuild every edited cart in the in-memory shelf from cached
                // library metadata. No ROM rescan, game saves or state edits.
                try {
                    val cache = File(filesDir, ROM_CACHE_NAME)
                    val json = JSONObject(cache.readText(Charsets.UTF_8))
                        .getJSONArray("games").toString()
                    if (setStudioOverrides(json) < 0) error("Studio library reload failed")
                    // nativeSetLibrary refreshes *all* GBA/GB/GBC label and shell
                    // material textures; keep the selected-cart path for focus.
                    if (selected != null) nativeReloadCartLabel(selected)
                } catch (error: Exception) {
                    Log.e(TAG, "Studio changes could not refresh shelf", error)
                    SlotToast.makeText(this, "Cart Studio changes saved. Reopen slot. to refresh artwork.",
                        Toast.LENGTH_LONG).show()
                }
            }
            return
        }
        if (requestCode == LABEL_REQUEST) {
            val selected = pendingCartLabelUri
            pendingCartLabelUri = null
            if (resultCode == RESULT_OK && selected != null) {
                data?.data?.let { importCartLabel(it, selected) }
            }
            return
        }
        if (requestCode == THEME_REQUEST || requestCode == WALLPAPER_REQUEST) {
            if (resultCode == RESULT_OK) data?.data?.let { importVisualFile(requestCode, it) }
            return
        }
        if (requestCode !in setOf(FOLDER_REQUEST, BIOS_REQUEST,
                SAVE_REQUEST, STATE_REQUEST) || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        try {
            val writable = requestCode == SAVE_REQUEST || requestCode == STATE_REQUEST
            val flags = if (writable) {
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            } else {
                Intent.FLAG_GRANT_READ_URI_PERMISSION
            }
            contentResolver.takePersistableUriPermission(uri, flags)
            when (requestCode) {
                FOLDER_REQUEST -> {
                    getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                        .putString(ROM_ROOT, uri.toString()).apply()
                    scanFolder(uri)
                }
                BIOS_REQUEST -> importBiosFolder(uri, persistSelection = true)
                SAVE_REQUEST, STATE_REQUEST -> {
                    val key = if (requestCode == SAVE_REQUEST) SAVE_ROOT else STATE_ROOT
                    getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                        .putString(key, uri.toString()).apply()
                    val kind = if (requestCode == SAVE_REQUEST) "Save" else "Save State"
                    SlotToast.makeText(this,
                        "$kind folder selected. RetroArch core subfolders will be used.",
                        Toast.LENGTH_LONG).show()
                }
            }
        } catch (error: SecurityException) {
            if (requestCode == FOLDER_REQUEST) {
                nativeSetShelfStatus(shelfPermissionError)
            } else {
                SlotToast.makeText(this, "This folder needs storage access. Please select it again.", Toast.LENGTH_LONG).show()
            }
            Log.e(TAG, "Android folder permission error", error)
        }
    }

    /** Read only a cache that belongs to this persisted SAF tree. */
    private fun cachedLibraryGames(root: Uri): String? {
        val file = File(filesDir, ROM_CACHE_NAME)
        if (!file.isFile || file.length() !in 1L..ROM_CACHE_MAX_BYTES.toLong()) return null
        return try {
            val metadata = JSONObject(file.readText(Charsets.UTF_8))
            if (metadata.optInt("schema") != ROM_CACHE_SCHEMA ||
                metadata.optString("root") != root.toString()) null
            else metadata.optJSONArray("games")?.toString()
        } catch (error: Exception) {
            Log.w(TAG, "Invalid ROM cache; scanning without replacing it first", error)
            null
        }
    }

    /** Reuse cached labels immediately without rescanning on the UI thread. */
    private fun restoreCachedLibrary(root: Uri): Boolean {
        val games = cachedLibraryGames(root) ?: return false
        return try {
            val count = setStudioOverrides(games)
            if (count < 0) return false
            nativeSetShelfStatus(if (count == 0) shelfEmpty else shelfNone)
            Log.i(TAG, "Restored $count cached ROMs without SAF rescan")
            true
        } catch (error: Exception) {
            Log.w(TAG, "Cached library unavailable; scanning", error)
            false
        }
    }

    /** AtomicFile retains the previous library if the write fails. */
    private fun storeCachedLibrary(root: Uri, json: String) {
        val payload = JSONObject()
            .put("schema", ROM_CACHE_SCHEMA)
            .put("root", root.toString())
            .put("games", JSONArray(json))
            .toString().toByteArray(Charsets.UTF_8)
        if (payload.size > ROM_CACHE_MAX_BYTES) {
            Log.w(TAG, "ROM library cache exceeds safe limit")
            return
        }
        val atomic = AtomicFile(File(filesDir, ROM_CACHE_NAME))
        val stream = atomic.startWrite()
        try {
            stream.write(payload)
            atomic.finishWrite(stream)
        } catch (error: Exception) {
            atomic.failWrite(stream)
            throw error
        }
    }

    /** The scanner never publishes directly. This method runs on the UI
     * thread only. Rust also rechecks the idle state at the exact GL swap,
     * guarding against a cartridge inserted between the two callbacks.
     */
    private fun publishPendingLibraryIfIdle() {
        val next = pendingLibraryIndex ?: return
        if (scanSerial.get() != next.serial) {
            pendingLibraryIndex = null
            libraryRefreshIndicator.hide()
            return
        }
        if (!resumed || !nativeCanRefreshLibrary()) return
        val saved = getSharedPreferences(PREFS, MODE_PRIVATE).getString(ROM_ROOT, null)
        if (saved != next.root.toString()) {
            pendingLibraryIndex = null
            libraryRefreshIndicator.hide()
            return
        }
        pendingLibraryIndex = null
        libraryRefreshIndicator.show()
        val count = try { nativeSetLibrary(next.prepared) } catch (error: Exception) {
            Log.e(TAG, "Changed ROM library could not be applied", error)
            -1
        }
        if (count < 0) {
            libraryRefreshIndicator.hide()
            nativeSetShelfStatus(shelfImportError)
            return
        }
        nativeSetShelfStatus(if (count == 0) shelfEmpty else shelfNone)
        // Index persistence and RA diagnostics remain on background workers.
        scanner.execute {
            if (scanSerial.get() != next.serial) return@execute
            try {
                storeCachedLibrary(next.root, next.json)
                startRaSmartCache()
                startAutoCartLabels()
            } catch (error: Exception) {
                Log.w(TAG, "ROM cache write failed; displayed shelf is intact", error)
            }
        }
        if (!next.quiet) {
            SlotToast.makeText(this, "Library updated: $count carts",
                Toast.LENGTH_SHORT).show()
        }
        // Brief subtle spinner only for a changed library; never for
        // unchanged boot scans and never while a game is active.
        uiHandler.postDelayed({
            if (::libraryRefreshIndicator.isInitialized && !isDestroyed &&
                pendingLibraryIndex == null) libraryRefreshIndicator.hide()
        }, 700L)
    }

    private fun scanFolder(uri: Uri, quiet: Boolean = false) {
        val serial = scanSerial.incrementAndGet()
        pendingLibraryIndex = null
        if (::libraryRefreshIndicator.isInitialized) libraryRefreshIndicator.hide()
        if (!quiet) nativeSetShelfStatus(shelfScanning)
        scanner.execute {
            try {
                val previous = cachedLibraryGames(uri)
                val result = RomLibrary.scan(this, uri, previous)
                if (scanSerial.get() != serial) return@execute
                // A bounded/incomplete SAF scan is not evidence that a ROM
                // was removed. Fail closed, retaining the previous shelf.
                if (result.truncated) {
                    Log.w(TAG, "Incomplete SAF ROM listing: retaining existing index")
                    runOnUiThread {
                        if (!isDestroyed && scanSerial.get() == serial && !quiet) {
                            nativeSetShelfStatus(shelfNone)
                            SlotToast.makeText(this,
                                "ROM scan incomplete. Existing games kept.",
                                Toast.LENGTH_LONG).show()
                        }
                    }
                    return@execute
                }
                if (previous != null && RomLibrary.sameIndex(previous, result.json)) {
                    Log.i(TAG, "ROM library unchanged: " + result.count +
                        " carts; no renderer rebuild")
                    runOnUiThread {
                        if (!isDestroyed && scanSerial.get() == serial && !quiet) {
                            nativeSetShelfStatus(if (result.count == 0) shelfEmpty else shelfNone)
                            SlotToast.makeText(this, "Library already up to date.",
                                Toast.LENGTH_SHORT).show()
                        }
                    }
                    return@execute
                }
                // Process artwork metadata before handing a changed index
                // to the UI. No game/ROM bytes or state files are touched.
                val prepared = CartStudioCatalog.withOverrides(this, result.json)
                runOnUiThread {
                    if (isDestroyed || scanSerial.get() != serial) return@runOnUiThread
                    pendingLibraryIndex = PendingLibraryIndex(
                        uri, result.json, prepared, serial, quiet)
                    publishPendingLibraryIfIdle()
                }
            } catch (error: Exception) {
                Log.e(TAG, "SAF ROM scan failed; previous cache retained", error)
                runOnUiThread {
                    if (!isDestroyed && scanSerial.get() == serial && !quiet)
                        nativeSetShelfStatus(shelfReadError)
                }
            }
        }
    }

    override fun onDestroy() {
        releaseLinkMulticast()
        Choreographer.getInstance().removeFrameCallback(vsyncCallback)
        raConnectivity?.let { manager ->
            try { manager.unregisterNetworkCallback(raConnectivityCallback) }
            catch (_: Exception) { /* Callback may have already been removed. */ }
        }
        raConnectivity = null
        scanSerial.incrementAndGet()
        pendingLibraryIndex = null
        if (::libraryRefreshIndicator.isInitialized) libraryRefreshIndicator.hide()
        scanner.shutdownNow()
        raLoginWorker.shutdownNow()
        raSmartWorker.shutdownNow()
        gameLoader.shutdownNow()
        romWarmWorker.shutdownNow()
        autoLabelsWorker.shutdownNow()
        uiHandler.removeCallbacks(pulse)
        synchronized(raSmartForeground) { raSmartForeground.notifyAll() }
        if (::raAchievementOverlay.isInitialized) raAchievementOverlay.destroy()
        if (::shelfStatus.isInitialized) shelfStatus.close()
        stopAudio()
        cartSounds.release()
        super.onDestroy()
    }

    override fun onPause() {
        resumed = false
        warmedCandidate.set(null)
        uiHandler.removeCallbacks(pulse)
        carouselDozing = false
        // Rust nativeSuspend aborts discovery/linked play; never keep a
        // multicast lock across Android background sleep.
        releaseLinkMulticast()
        if (::shelfStatus.isInitialized) shelfStatus.onPause()
        if (::autoLabelIndicator.isInitialized) autoLabelIndicator.hide()
        Choreographer.getInstance().removeFrameCallback(vsyncCallback)
        if (displaySyncApplied) {
            view.renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
            displaySyncApplied = false
        }
        stopAudio()
        cartSounds.pause()
        // Preserve SRAM and automatic resume state while the GL context is still current.
        val complete = CountDownLatch(1)
        view.queueEvent { try { nativeSuspend() } finally { complete.countDown() } }
        try {
            if (!complete.await(2, TimeUnit.SECONDS)) Log.w(TAG, "Suspend save timed out")
        } catch (_: InterruptedException) {
            Thread.currentThread().interrupt()
        }
        view.onPause()
        // The final SRAM write above can enqueue a RetroArch/Sync export.
        drainPendingSaveFlush()
        nativeResetInput()
        super.onPause()
    }

    override fun onResume() {
        super.onResume()
        cartStudioOpening = false
        if (returnFromSlotChild) {
            // Clear the preserved native menu ONLY after Apps/Studio has
            // finished and the shelf itself is about to be visible again.
            returnFromSlotChild = false
            nativeExternalReturn()
        }
        resumed = true
        synchronized(raSmartForeground) { raSmartForeground.notifyAll() }
        carouselDozing = false
        view.renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
        uiHandler.removeCallbacks(pulse)
        uiHandler.post(pulse)
        if (::shelfStatus.isInitialized) shelfStatus.onResume()
        // Android's quick settings may return without a fresh onAvailable().
        // Waking a retry is safe even when no network is present.
        nativeRaNetworkAvailable()
        if (RaAccount.configuredUser(filesDir) != null) {
            nativeRaAccountConfigured()
            startRaSmartCache()
        }
        startAutoCartLabels()
        slotUpdater.onForeground()
        lastStatusPoll = -15000L
        cartSounds.resume()
        view.onResume()
        updateDisplaySyncMode()
        immersive()
    }

    private fun immersive() {
        @Suppress("DEPRECATION")
        window.decorView.systemUiVisibility = (
            View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
            View.SYSTEM_UI_FLAG_FULLSCREEN or
            View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
            View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
            View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
            View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        )
    }
}
