package fyi.slot.konkr

import android.content.Context
import android.opengl.GLSurfaceView
import android.view.SurfaceView

/** GLSurfaceView presenter. All native session commands run on its GL thread. */
internal class SlotPresenter(context: Context, renderer: GLSurfaceView.Renderer) {
    private val gl = GLSurfaceView(context).apply {
        setEGLContextClientVersion(2)
        preserveEGLContextOnPause = true
        setRenderer(renderer)
        renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
        isFocusableInTouchMode = true
        requestFocus()
    }
    val surfaceView: SurfaceView get() = gl
    var renderMode: Int
        get() = gl.renderMode
        set(value) { gl.renderMode = value }
    fun requestRender() = gl.requestRender()
    fun queueEvent(command: Runnable) = gl.queueEvent(command)
    fun onPause() = gl.onPause()
    fun onResume() = gl.onResume()
    // GLSurfaceView releases its EGL context itself on detach.
    fun close() = Unit
}
