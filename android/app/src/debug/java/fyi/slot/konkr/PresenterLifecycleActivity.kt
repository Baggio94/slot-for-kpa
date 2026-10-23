package fyi.slot.konkr

import android.app.Activity
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import android.os.Bundle
import android.widget.FrameLayout
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10

/** Instrumentation-only host in the diagnostic package. Does not load ROMs/JNI. */
class PresenterLifecycleActivity : Activity() {
    internal lateinit var presenter: ControlledEglPresenter
    lateinit var container: FrameLayout
    val creates = AtomicInteger()
    val frames = AtomicInteger()
    val ready = AtomicBoolean(false)
    val ownerThreads = ConcurrentLinkedQueue<Long>()
    val events = ConcurrentLinkedQueue<String>()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        presenter = ControlledEglPresenter(this, object : GLSurfaceView.Renderer {
            override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
                ownerThreads.add(Thread.currentThread().id)
                creates.incrementAndGet(); ready.set(true)
            }
            override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
                GLES20.glViewport(0, 0, width, height)
            }
            override fun onDrawFrame(gl: GL10?) {
                GLES20.glClearColor(0.25f, 0.5f, 0.75f, 1f)
                GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT)
                check(GLES20.glGetError() == GLES20.GL_NO_ERROR)
                frames.incrementAndGet()
            }
        }, beforeFrame = {}, contextLost = { ready.set(false); events.add("lost") },
            resumedClock = {}, shutdown = { current ->
                ownerThreads.add(Thread.currentThread().id)
                events.add("shutdown:$current"); ready.set(false)
            }, nativeReady = { ready.get() })
        container = FrameLayout(this)
        container.addView(presenter.surfaceView)
        setContentView(container)
    }
    override fun onResume() { super.onResume(); presenter.onResume() }
    override fun onPause() { presenter.onPause(); super.onPause() }
    override fun onDestroy() { presenter.close(); super.onDestroy() }
}
