package app.watchdog.remote

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.StatFs

/**
 * Cached battery and storage readouts for STATE frames. Queries are throttled
 * because the state timer emits at up to 30 Hz and both system calls are
 * comparatively expensive.
 */
class DeviceReadouts(private val context: Context) {
    @Volatile
    private var batteryPercent = 0

    @Volatile
    private var lastBatteryQueryMs = 0L

    @Volatile
    private var storageFreeMb = 0L

    @Volatile
    private var lastStorageQueryMs = 0L

    @Volatile
    private var batteryTemperatureC: Double? = null

    @Volatile
    private var lastTemperatureQueryMs = 0L

    fun batteryPercent(): Int {
        val now = System.currentTimeMillis()
        if (lastBatteryQueryMs == 0L || now - lastBatteryQueryMs >= QUERY_INTERVAL_MS) {
            val manager = context.getSystemService(Context.BATTERY_SERVICE) as BatteryManager
            batteryPercent = manager.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
            lastBatteryQueryMs = now
        }
        return batteryPercent
    }

    fun storageFreeMb(): Long {
        val now = System.currentTimeMillis()
        if (lastStorageQueryMs == 0L || now - lastStorageQueryMs >= QUERY_INTERVAL_MS) {
            storageFreeMb = StatFs(context.filesDir.absolutePath).availableBytes / BYTES_PER_MB
            lastStorageQueryMs = now
        }
        return storageFreeMb
    }

    /** Battery temperature is the only thermal signal available on every API 26+ device. */
    fun temperatureC(): Double? {
        val now = System.currentTimeMillis()
        if (lastTemperatureQueryMs == 0L || now - lastTemperatureQueryMs >= QUERY_INTERVAL_MS) {
            val intent = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
            val tenths = intent?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, ABSENT_TEMPERATURE)
            batteryTemperatureC = tenths?.takeIf { it >= 0 }?.div(TENTHS_PER_CELSIUS)
            lastTemperatureQueryMs = now
        }
        return batteryTemperatureC
    }

    private companion object {
        const val QUERY_INTERVAL_MS = 5_000L
        const val BYTES_PER_MB = 1024L * 1024L
        const val ABSENT_TEMPERATURE = -1
        const val TENTHS_PER_CELSIUS = 10.0
    }
}
