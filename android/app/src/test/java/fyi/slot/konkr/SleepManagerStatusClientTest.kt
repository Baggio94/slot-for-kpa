package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SleepManagerStatusClientTest {
    @Test
    fun decodesEnabledAndServiceStateIndependently() {
        val running = SleepManagerStatusClient.parse(
            """{"apiVersion":1,"enabled":true,"serviceRunning":true}"""
        )
        assertEquals(SleepManagerStatusClient.Status(true, true), running)

        val waiting = SleepManagerStatusClient.parse(
            """{"apiVersion":1,"enabled":true,"serviceRunning":false}"""
        )
        assertEquals(SleepManagerStatusClient.Status(true, false), waiting)
        assertEquals(
            SleepManagerStatusClient.Status(false, false),
            SleepManagerStatusClient.parse(
                """{"apiVersion":1,"enabled":false,"serviceRunning":false}"""
            )
        )
    }

    @Test
    fun unsupportedOrIncompleteResponsesRemainUnknown() {
        val payloads = listOf(
            null, "", "invalid",
            """{"apiVersion":2,"enabled":true,"serviceRunning":true}""",
            """{"apiVersion":1,"enabled":true}""",
            """{"apiVersion":1,"enabled":"true","serviceRunning":true}""",
            """{"apiVersion":1,"enabled":false,"serviceRunning":null}"""
        )
        payloads.forEach { payload ->
            assertNull(payload, SleepManagerStatusClient.parse(payload))
        }
    }
}
