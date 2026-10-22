package fyi.slot.konkr

import org.junit.Assert.*
import org.junit.Test

class PresenterScheduleTest {
    @Test fun burstOfRequestsCannotBuildAFrameBacklog() {
        val schedule = PresenterSchedule().apply { resumed = true; surfaceReady = true; continuous = false }
        assertTrue(schedule.schedule())
        repeat(10_000) { schedule.request(); assertFalse(schedule.schedule()) }
        assertTrue(schedule.frame())
        assertFalse(schedule.frame())
        assertFalse(schedule.schedule())
        schedule.request()
        assertTrue(schedule.schedule())
    }
    @Test fun pauseOrDetachRejectsPendingFrameAndResumeHasNoCatchupBurst() {
        val schedule = PresenterSchedule().apply { resumed = true; surfaceReady = true }
        assertTrue(schedule.schedule())
        schedule.resumed = false
        assertFalse(schedule.frame())
        repeat(100) { schedule.request(); assertFalse(schedule.schedule()) }
        schedule.resumed = true
        schedule.surfaceReady = false
        assertFalse(schedule.schedule())
        schedule.surfaceReady = true
        assertTrue(schedule.schedule())
        assertFalse(schedule.schedule())
        assertTrue(schedule.frame())
    }
    @Test fun dirtyRequestSurvivesSurfaceAbsenceAndCloseIsTerminal() {
        val schedule = PresenterSchedule().apply { continuous = false; resumed = true }
        schedule.request()
        assertFalse(schedule.schedule())
        schedule.surfaceReady = true
        assertTrue(schedule.schedule())
        schedule.cancel()
        assertFalse(schedule.frame())
        assertTrue(schedule.schedule())
        schedule.close()
        schedule.request()
        schedule.resumed = true
        schedule.surfaceReady = true
        assertFalse(schedule.schedule())
        assertFalse(schedule.frame())
    }
}
