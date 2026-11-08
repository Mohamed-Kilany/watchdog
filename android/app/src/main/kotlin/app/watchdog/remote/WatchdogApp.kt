package app.watchdog.remote

import android.app.Application
import app.watchdog.remote.log.DeviceLog

/** Enables logcat mirroring for the process-wide [DeviceLog] ring. */
class WatchdogApp : Application() {
    override fun onCreate() {
        super.onCreate()
        DeviceLog.logcatEnabled = true
    }
}
