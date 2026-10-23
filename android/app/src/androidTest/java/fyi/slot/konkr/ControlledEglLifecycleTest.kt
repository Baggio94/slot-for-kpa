package fyi.slot.konkr

import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Uses Android's actual SurfaceHolder, HandlerThread, Choreographer and EGL14. */
@RunWith(AndroidJUnit4::class)
class ControlledEglLifecycleTest {
    private fun await(message: String, condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000L
        while (!condition() && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(10)
        assertTrue(message, condition())
    }
    @Test fun detachPauseAndShutdownKeepOneOwnerAndNeverRenderWithoutASurface() {
        val scenario = ActivityScenario.launch(PresenterLifecycleActivity::class.java)
        lateinit var activity: PresenterLifecycleActivity
        try {
            scenario.onActivity { activity = it }
            await("initial real EGL frames") { activity.frames.get() >= 3 }
            assertEquals(1, activity.creates.get())
            scenario.onActivity { it.container.removeView(it.presenter.surfaceView) }
            // The surfaceDestroyed barrier must have completed before returning.
            val detachedFrames = activity.frames.get()
            val commands = CountDownLatch(100)
            repeat(100) { i -> activity.presenter.queueEvent {
                activity.ownerThreads.add(Thread.currentThread().id)
                activity.events.add("command:$i"); commands.countDown()
            } }
            assertTrue(commands.await(5, TimeUnit.SECONDS))
            SystemClock.sleep(80)
            assertEquals(detachedFrames, activity.frames.get())
            assertEquals((0 until 100).map { "command:$it" }, activity.events.filter { it.startsWith("command:") })
            scenario.onActivity { it.container.addView(it.presenter.surfaceView) }
            await("frames resume on replacement surface") { activity.frames.get() >= detachedFrames + 3 }
            assertEquals("context retained without native reinitialization", 1, activity.creates.get())
            scenario.onActivity { it.presenter.onPause() }
            val pausedFrames = activity.frames.get()
            repeat(1000) { activity.presenter.requestRender() }
            SystemClock.sleep(80)
            assertEquals(pausedFrames, activity.frames.get())
            scenario.onActivity { it.presenter.onResume() }
            await("frames resume after pause") { activity.frames.get() > pausedFrames }
        } finally { scenario.close() }
        assertTrue(activity.events.contains("shutdown:true"))
        assertFalse(activity.events.contains("lost"))
        assertEquals("one native owner through attach/detach/shutdown", 1, activity.ownerThreads.toSet().size)
        val closedFrames = activity.frames.get()
        activity.presenter.requestRender()
        SystemClock.sleep(80)
        assertEquals(closedFrames, activity.frames.get())
    }
}
