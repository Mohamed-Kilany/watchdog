package app.watchdog.remote.camera

import org.junit.Assert.assertEquals
import org.junit.Test

/** M4-T08 flash and torch readouts. */
class CameraControlsTest {
    @Test
    fun `flash mode drives the flash and torch actuals`() {
        assertEquals(mapOf("flash_mode" to 0, "torch" to false), CameraControls.flashReadouts(0))
        assertEquals(mapOf("flash_mode" to 1, "torch" to false), CameraControls.flashReadouts(1))
        assertEquals(mapOf("flash_mode" to 2, "torch" to true), CameraControls.flashReadouts(2))
    }
}
