package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RaConfigCodecTest {
    @Test fun tokenConfigHasNoPasswordAndDisablesNoGameData() {
        val config = RaConfigCodec.configuredAccount("Player", "session-abc")
        assertTrue(config.startsWith("enabled = true\n"))
        assertTrue(config.contains("username = \"Player\"\n"))
        assertTrue(config.contains("token = \"session-abc\"\n"))
        assertTrue(config.contains("password = \"\"\n"))
        assertFalse(config.contains("/ROMs"))
        assertEquals("Player", RaConfigCodec.displayedUsername(config))
    }

    @Test fun escapingCannotAddConfigurationFields() {
        val config = RaConfigCodec.configuredAccount("A\"\\B", "token\"\\C")
        assertTrue(config.contains("username = \"A\\\"\\\\B\""))
        assertTrue(config.contains("token = \"token\\\"\\\\C\""))
        assertEquals("A\"\\B", RaConfigCodec.displayedUsername(config))
    }

    @Test(expected = IllegalArgumentException::class)
    fun newlineInjectionIsRejected() {
        RaConfigCodec.configuredAccount("Player\npassword = \"evil\"", "token")
    }

    @Test(expected = IllegalArgumentException::class)
    fun emptyTokenIsRejected() {
        RaConfigCodec.configuredAccount("Player", "")
    }

    @Test fun malformedAccountCannotBeDisplayed() {
        assertEquals(null, RaConfigCodec.displayedUsername("username = \"unfinished"))
    }
}
