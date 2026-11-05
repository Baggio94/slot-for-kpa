package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Test

class SlotDeviceTest {
    @Test
    fun aModelThatReadsAsANameStandsAlone() {
        // The Retroid Pocket Classic's maker is reported as Moorechip.
        assertEquals("Retroid Pocket Classic",
            SlotDevice.displayName("Moorechip", "Retroid Pocket Classic"))
    }

    @Test
    fun aBareModelCodeGetsItsMaker() {
        assertEquals("Anbernic RG405M", SlotDevice.displayName("Anbernic", "RG405M"))
        assertEquals("AYN Thor", SlotDevice.displayName("AYN", "Thor"))
    }

    @Test
    fun theMakerIsNeverRepeatedOrInvented() {
        assertEquals("AYANEO Pocket Micro", SlotDevice.displayName("AYANEO", "AYANEO Pocket Micro"))
        assertEquals("RG405M", SlotDevice.displayName("", "RG405M"))
        assertEquals("Anbernic", SlotDevice.displayName("Anbernic", ""))
    }
}
