package app.watchdog.remote.transport

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.StatFs
import app.watchdog.remote.BuildConfig
import app.watchdog.remote.camera.CameraCatalog
import app.watchdog.remote.camera.CameraInfo
import org.json.JSONArray
import org.json.JSONObject

/**
 * Builds the `HELLO_ACK` payload (`docs/protocol.md` §5.2): app identity,
 * device information, camera list and per-camera capabilities. Capability
 * descriptors are filled in M4 (control introspection); until then the map is
 * empty and the host treats controls as unsupported.
 */
object HelloAckBuilder {
    fun build(context: Context, protocolVersion: Int, capabilities: JSONObject = JSONObject()): ByteArray {
        val app =
            JSONObject()
                .put("version_code", BuildConfig.VERSION_CODE)
                .put("version_name", BuildConfig.VERSION_NAME)
                .put("package", context.packageName)
        val device =
            JSONObject()
                .put("manufacturer", Build.MANUFACTURER)
                .put("model", Build.MODEL)
                .put("device", Build.DEVICE)
                .put("sdk", Build.VERSION.SDK_INT)
                .put("release", Build.VERSION.RELEASE)
                .put("abi", Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown")
                .put("battery_percent", batteryPercent(context))
                .put("storage_free_mb", storageFreeMb(context))
                .put("temperature_c", batteryTemperatureC(context))
        val cameras = JSONArray()
        CameraCatalog.list(context).forEach { camera -> cameras.put(cameraJson(camera)) }
        return JSONObject()
            .put("protocol_version", protocolVersion)
            .put("app", app)
            .put("device", device)
            .put("cameras", cameras)
            .put("capabilities", capabilities)
            .toString()
            .toByteArray(Charsets.UTF_8)
    }

    private fun cameraJson(camera: CameraInfo): JSONObject {
        val physical = JSONArray()
        camera.physicalIds.forEach { physical.put(it) }
        return JSONObject()
            .put("id", camera.id)
            .put("facing", camera.facing)
            .put("hardware_level", camera.hardwareLevel)
            .put("logical_multi_camera", camera.logicalMultiCamera)
            .put("physical_ids", physical)
    }

    private fun batteryPercent(context: Context): Int {
        val manager = context.getSystemService(Context.BATTERY_SERVICE) as BatteryManager
        return manager.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
    }

    private fun storageFreeMb(context: Context): Long {
        val stat = StatFs(context.filesDir.absolutePath)
        return stat.availableBytes / BYTES_PER_MB
    }

    private fun batteryTemperatureC(context: Context): Double? {
        val intent = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val tenths = intent?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, ABSENT_TEMPERATURE) ?: ABSENT_TEMPERATURE
        return if (tenths >= 0) tenths / TENTHS_PER_CELSIUS else null
    }

    private const val BYTES_PER_MB = 1024L * 1024L
    private const val ABSENT_TEMPERATURE = -1
    private const val TENTHS_PER_CELSIUS = 10.0
}
