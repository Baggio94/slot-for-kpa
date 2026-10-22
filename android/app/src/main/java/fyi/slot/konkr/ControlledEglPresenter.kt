package fyi.slot.konkr

import android.content.Context
import android.opengl.EGL14
import android.opengl.EGLConfig
import android.opengl.EGLContext
import android.opengl.EGLDisplay
import android.opengl.EGLSurface
import android.opengl.GLSurfaceView
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicBoolean

/** Experimental GLES presenter. One persistent owner per Activity, including
 * periods with no window. EGL swaps are measured here, never called present time.
 * The 1x1 pbuffer keeps the context current for ordered native lifecycle commands.
 */
internal class ControlledEglPresenter(
    context: Context,
    private val renderer: GLSurfaceView.Renderer,
    private val beforeFrame: (Long) -> Unit,
    private val contextLost: () -> Unit,
    private val resumedClock: () -> Unit,
    private val shutdown: (Boolean) -> Unit,
    private val nativeReady: () -> Boolean,
) : SlotPresenter, SurfaceHolder.Callback {
    companion object { private const val TAG = "SlotKonkr" }
    override val ownsScheduler = true
    override val surfaceView = SurfaceView(context).apply {
        isFocusableInTouchMode = true
        requestFocus()
    }
    private val thread = HandlerThread("slot-egl-owner").apply { start() }
    private val owner = Handler(thread.looper)
    private val closed = AtomicBoolean(false)
    private val admission = Any()
    private val renderMessage = AtomicBoolean(false)
    private val schedule = PresenterSchedule()
    private lateinit var choreographer: Choreographer
    private var display: EGLDisplay = EGL14.EGL_NO_DISPLAY
    private var eglContext: EGLContext = EGL14.EGL_NO_CONTEXT
    private var config: EGLConfig? = null
    private var parking: EGLSurface = EGL14.EGL_NO_SURFACE
    private var window: EGLSurface = EGL14.EGL_NO_SURFACE
    private var androidSurface: Surface? = null
    private var width = 0
    private var height = 0
    private var epoch = 0L
    private var frameId = 0L
    private var reportAt = 0L
    private var swaps = 0L
    private var swapTotalUs = 0L
    private var swapMaxUs = 0L
    private var swapOver16 = 0L
    @Volatile private var mode = GLSurfaceView.RENDERMODE_CONTINUOUSLY

    init {
        owner.post { choreographer = Choreographer.getInstance() }
        surfaceView.holder.addCallback(this)
    }

    override var renderMode: Int
        get() = mode
        set(value) {
            require(value == GLSurfaceView.RENDERMODE_CONTINUOUSLY || value == GLSurfaceView.RENDERMODE_WHEN_DIRTY)
            mode = value
            post { schedule.continuous = value == GLSurfaceView.RENDERMODE_CONTINUOUSLY; kick() }
        }

    override fun requestRender() {
        // At most one request message even if UI/input pulses outrun EGL. Native
        // input itself lives in its ordered queue and is never discarded here.
        if (closed.get() || !renderMessage.compareAndSet(false, true)) return
        post { renderMessage.set(false); schedule.request(); kick() }
    }
    override fun queueEvent(command: Runnable) { post { command.run() } }
    override fun onPause() = barrier {
        schedule.resumed = false
        cancelFrame()
    }
    override fun onResume() { post {
        schedule.resumed = true
        if (!schedule.surfaceReady && androidSurface != null) attach()
        resumedClock()
        schedule.request()
        kick()
    } }

    override fun surfaceCreated(holder: SurfaceHolder) {
        val surface = holder.surface
        post { androidSurface = surface; width = 0; height = 0 }
    }
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        val surface = holder.surface
        post {
            androidSurface = surface
            this.width = width
            this.height = height
            if (width <= 0 || height <= 0) {
                cancelFrame(); schedule.surfaceReady = false
                return@post
            }
            if (window == EGL14.EGL_NO_SURFACE || !schedule.surfaceReady) attach()
            else renderer.onSurfaceChanged(null, width, height)
            schedule.request()
            kick()
        }
    }
    override fun surfaceDestroyed(holder: SurfaceHolder) = barrier {
        // Android requires the render thread to stop touching this Surface
        // before this callback returns. Never time out and keep drawing on it.
        cancelFrame()
        schedule.surfaceReady = false
        androidSurface = null
        detachWindow()
        Log.i(TAG, "EGL owner surface detached: epoch=$epoch thread=${Thread.currentThread().id}")
    }

    private val frame: Choreographer.FrameCallback = Choreographer.FrameCallback { timestamp ->
        if (schedule.frame()) {
            try {
                beforeFrame(timestamp)
                renderer.onDrawFrame(null)
                val started = SystemClock.elapsedRealtimeNanos()
                checkEgl(EGL14.eglSwapBuffers(display, window), "swap")
                val us = (SystemClock.elapsedRealtimeNanos() - started) / 1000L
                frameId++
                swaps++
                swapTotalUs += us
                swapMaxUs = maxOf(swapMaxUs, us)
                if (us > 16_667L) swapOver16++
                if (reportAt == 0L) reportAt = started
                if (started - reportAt >= 5_000_000_000L) {
                    Log.i(TAG, "EGL owner swap: epoch=$epoch submitted_id=$frameId swaps=$swaps " +
                        "swap_mean_us=${swapTotalUs / swaps} swap_max_us=$swapMaxUs " +
                        "swap_over16=$swapOver16 pending_frame_requests_max=1 not_actual_present_time=true")
                    reportAt = started; swaps = 0; swapTotalUs = 0; swapMaxUs = 0; swapOver16 = 0
                }
            } catch (error: RuntimeException) { fail(error) }
        }
        kick()
    }
    private fun kick() { if (schedule.schedule()) choreographer.postFrameCallback(frame) }
    private fun cancelFrame() { choreographer.removeFrameCallback(frame); schedule.cancel() }

    private fun attach() {
        val surface = androidSurface ?: return
        if (!surface.isValid || width <= 0 || height <= 0) return
        cancelFrame()
        schedule.surfaceReady = false
        try {
            if (display == EGL14.EGL_NO_DISPLAY) initialize()
            detachWindow()
            window = EGL14.eglCreateWindowSurface(display, config, surface, intArrayOf(EGL14.EGL_NONE), 0)
            checkEgl(window != EGL14.EGL_NO_SURFACE, "create window")
            checkEgl(EGL14.eglMakeCurrent(display, window, window, eglContext), "make window current")
            checkEgl(EGL14.eglSwapInterval(display, 1), "swap interval")
            // A retained context only needs a new window; never rebuild GPU
            // objects in the same context or reload the live emulation core.
            if (!nativeReady()) {
                renderer.onSurfaceCreated(null, null)
                check(nativeReady()) { "native compositor reconstruction failed" }
            }
            renderer.onSurfaceChanged(null, width, height)
            resumedClock()
            epoch++
            reportAt = 0L; swaps = 0; swapTotalUs = 0; swapMaxUs = 0; swapOver16 = 0
            schedule.surfaceReady = true
            Log.i(TAG, "EGL owner surface attached: epoch=$epoch size=${width}x$height " +
                "thread=${Thread.currentThread().id} swap_interval=1")
        } catch (error: RuntimeException) { fail(error) }
    }

    private fun initialize() {
        display = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
        checkEgl(display != EGL14.EGL_NO_DISPLAY, "get display")
        val version = IntArray(2)
        checkEgl(EGL14.eglInitialize(display, version, 0, version, 1), "initialize")
        checkEgl(EGL14.eglBindAPI(EGL14.EGL_OPENGL_ES_API), "bind ES")
        val configs = arrayOfNulls<EGLConfig>(1)
        val count = IntArray(1)
        checkEgl(EGL14.eglChooseConfig(display, intArrayOf(
            EGL14.EGL_RENDERABLE_TYPE, EGL14.EGL_OPENGL_ES2_BIT,
            EGL14.EGL_SURFACE_TYPE, EGL14.EGL_WINDOW_BIT or EGL14.EGL_PBUFFER_BIT,
            EGL14.EGL_RED_SIZE, 8, EGL14.EGL_GREEN_SIZE, 8, EGL14.EGL_BLUE_SIZE, 8,
            EGL14.EGL_ALPHA_SIZE, 0, EGL14.EGL_DEPTH_SIZE, 0, EGL14.EGL_STENCIL_SIZE, 0,
            EGL14.EGL_NONE), 0, configs, 0, 1, count, 0) && count[0] > 0, "choose config")
        config = configs[0]
        eglContext = EGL14.eglCreateContext(display, config, EGL14.EGL_NO_CONTEXT,
            intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION, 2, EGL14.EGL_NONE), 0)
        checkEgl(eglContext != EGL14.EGL_NO_CONTEXT, "create context")
        parking = EGL14.eglCreatePbufferSurface(display, config,
            intArrayOf(EGL14.EGL_WIDTH, 1, EGL14.EGL_HEIGHT, 1, EGL14.EGL_NONE), 0)
        checkEgl(parking != EGL14.EGL_NO_SURFACE, "create parking surface")
    }
    private fun detachWindow() {
        if (window == EGL14.EGL_NO_SURFACE) return
        checkEgl(EGL14.eglMakeCurrent(display, parking, parking, eglContext), "park context")
        checkEgl(EGL14.eglDestroySurface(display, window), "destroy window")
        window = EGL14.EGL_NO_SURFACE
    }
    private fun fail(error: RuntimeException) {
        Log.e(TAG, "EGL owner suspended; session retained; retry on surface/resume", error)
        cancelFrame()
        schedule.surfaceReady = false
        contextLost() // Abandon GL names before the context is destroyed.
        destroyEgl()
    }
    private fun destroyEgl() {
        if (display != EGL14.EGL_NO_DISPLAY) {
            EGL14.eglMakeCurrent(display, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_CONTEXT)
            if (window != EGL14.EGL_NO_SURFACE) EGL14.eglDestroySurface(display, window)
            if (parking != EGL14.EGL_NO_SURFACE) EGL14.eglDestroySurface(display, parking)
            if (eglContext != EGL14.EGL_NO_CONTEXT) EGL14.eglDestroyContext(display, eglContext)
            EGL14.eglTerminate(display)
        }
        window = EGL14.EGL_NO_SURFACE; parking = EGL14.EGL_NO_SURFACE
        eglContext = EGL14.EGL_NO_CONTEXT; display = EGL14.EGL_NO_DISPLAY; config = null
    }
    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        surfaceView.holder.removeCallback(this)
        barrier(allowClosed = true) {
            cancelFrame(); schedule.close()
            try { shutdown(EGL14.eglGetCurrentContext() != EGL14.EGL_NO_CONTEXT) }
            finally { destroyEgl(); thread.quitSafely() }
        }
    }
    private fun post(command: () -> Unit) {
        synchronized(admission) {
            // Commands accepted before close must execute before shutdown.
            if (!closed.get()) owner.post { command() }
        }
    }
    private fun barrier(allowClosed: Boolean = false, command: () -> Unit) {
        if (closed.get() && !allowClosed) return
        if (Looper.myLooper() == thread.looper) { command(); return }
        val complete = CountDownLatch(1)
        synchronized(admission) {
            check(owner.post { try { command() } catch (error: RuntimeException) { fail(error) }
                finally { complete.countDown() } }) { "EGL owner unavailable" }
        }
        var interrupted = false
        while (true) {
            try { complete.await(); break } catch (_: InterruptedException) { interrupted = true }
        }
        if (interrupted) Thread.currentThread().interrupt()
    }
    private fun checkEgl(ok: Boolean, operation: String) {
        check(ok) { "$operation failed: EGL 0x${EGL14.eglGetError().toString(16)}" }
    }
}
