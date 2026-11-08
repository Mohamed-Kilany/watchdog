package app.watchdog.remote.camera

import android.content.Context
import android.hardware.camera2.CameraManager
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.watchdog.remote.transport.HelloAckBuilder
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * M4-T01: every camera must have a complete, self-describing capability
 * descriptor, and the handshake must carry exactly that map. Runs on the CI
 * emulator (virtual scene camera).
 */
@RunWith(AndroidJUnit4::class)
class CapabilityDescriptorTest {
    private val context: Context = ApplicationProvider.getApplicationContext()

    @Test
    fun everyCameraHasACompleteDescriptor() {
        val manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val descriptors = CapabilityDescriptor.all(context)
        assertTrue("emulator must expose at least one camera", manager.cameraIdList.isNotEmpty())
        assertEquals(manager.cameraIdList.size, descriptors.length())
        for (id in manager.cameraIdList) {
            val descriptor = descriptors.getJSONObject(id)
            if (descriptor.has("error")) {
                assertTrue("$id: error text", descriptor.getString("error").isNotEmpty())
                continue
            }
            assertTrue("$id: hardware_level", descriptor.has("hardware_level"))
            assertTrue("$id: lens_facing", descriptor.has("lens_facing"))
            assertTrue("$id: sensor_orientation", descriptor.has("sensor_orientation"))
            assertTrue("$id: capabilities", descriptor.getJSONArray("capabilities").length() > 0)
            assertTrue("$id: physical_ids", descriptor.has("physical_ids"))
            assertTrue("$id: lens", descriptor.has("lens"))
            assertTrue("$id: unsupported", descriptor.has("unsupported"))
            val streams = descriptor.getJSONObject("streams")
            assertTrue("$id: stream configuration", streams.getBoolean("available"))
            assertTrue("$id: output formats", streams.getJSONObject("formats").length() > 0)
            assertTrue("$id: fps ranges", streams.getJSONArray("fps_ranges").length() > 0)
            assertKeyInventory(descriptor, id, "available_request_keys")
            assertKeyInventory(descriptor, id, "available_result_keys")
            assertKeyInventory(descriptor, id, "available_characteristics_keys")
            assertControls(descriptor, id)
        }
    }

    @Test
    fun helloAckEmbedsTheDescriptorMap() {
        val descriptors = CapabilityDescriptor.all(context)
        val ack = JSONObject(String(HelloAckBuilder.build(context, 1, descriptors), Charsets.UTF_8))
        assertEquals(1, ack.getInt("protocol_version"))
        val embedded = ack.getJSONObject("capabilities")
        assertEquals(descriptors.length(), embedded.length())
        descriptors.keys().forEach { id -> assertTrue("missing camera $id", embedded.has(id)) }
    }

    private fun assertKeyInventory(descriptor: JSONObject, id: String, field: String) {
        val keys = descriptor.getJSONArray(field)
        assertTrue("$id: $field", keys.length() > 0)
        for (index in 0 until keys.length()) {
            val entry = keys.getJSONObject(index)
            assertTrue("$id: $field name", entry.getString("name").isNotEmpty())
            assertTrue("$id: $field type", entry.getString("type").isNotEmpty())
        }
    }

    private fun assertControls(descriptor: JSONObject, id: String) {
        val metadata =
            setOf(
                "hardware_level",
                "lens_facing",
                "sensor_orientation",
                "capabilities",
                "physical_ids",
                "lens",
                "streams",
                "available_request_keys",
                "available_result_keys",
                "available_characteristics_keys",
                "unsupported",
            )
        var supported = 0
        val names = descriptor.keys()
        while (names.hasNext()) {
            val name = names.next()
            if (name in metadata) {
                continue
            }
            val control = descriptor.getJSONObject(name)
            if (!control.getBoolean("supported")) {
                continue
            }
            supported++
            if (control.has("modes")) {
                assertTrue("$id: $name modes", control.getJSONArray("modes").length() > 0)
            } else if (control.has("min") && control.has("max")) {
                assertTrue("$id: $name range", control.getDouble("max") >= control.getDouble("min"))
                assertTrue("$id: $name step", control.getDouble("step") >= 0.0)
            }
        }
        assertTrue("$id: at least one supported control", supported > 0)
        val unsupported = descriptor.getJSONArray("unsupported")
        for (index in 0 until unsupported.length()) {
            val entry = unsupported.getJSONObject(index)
            assertTrue("$id: unsupported control name", entry.getString("control").isNotEmpty())
            assertTrue("$id: unsupported reason", entry.getString("reason").isNotEmpty())
        }
    }
}
