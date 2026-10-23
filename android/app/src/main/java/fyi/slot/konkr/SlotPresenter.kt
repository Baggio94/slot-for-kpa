package fyi.slot.konkr

import android.content.Context
import android.opengl.GLSurfaceView
import android.view.SurfaceView

/** All native session commands run on the chosen presenter's sole owner thread. */
internal interface SlotPresenter {
    val surfaceView: SurfaceView
    val ownsScheduler: Boolean
    var renderMode: Int
    fun requestRender()
    fun queueEvent(command: Runnable)
    fun onPause()
    fun onResume()
    fun close()
}

internal class ReferenceGlPresenter(context: Context, renderer: GLSurfaceView.Renderer) : SlotPresenter {
    private val gl = GLSurfaceView(context).apply {
        setEGLContextClientVersion(2)
        preserveEGLContextOnPause = true
        setRenderer(renderer)
        renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
        isFocusableInTouchMode = true
        requestFocus()
    }
    override val surfaceView: SurfaceView get() = gl
    override val ownsScheduler = false
    override var renderMode: Int
        get() = gl.renderMode
        set(value) { gl.renderMode = value }
    override fun requestRender() = gl.requestRender()
    override fun queueEvent(command: Runnable) = gl.queueEvent(command)
    override fun onPause() = gl.onPause()
    override fun onResume() = gl.onResume()
    // GLSurfaceView retains its existing shutdown behavior in the reference path.
    override fun close() = Unit
}
