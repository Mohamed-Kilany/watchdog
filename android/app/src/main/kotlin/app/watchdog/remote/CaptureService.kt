package app.watchdog.remote

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import app.watchdog.remote.log.DeviceLog
import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.Packet
import app.watchdog.remote.transport.HelloAckBuilder
import app.watchdog.remote.transport.LogStreamer
import app.watchdog.remote.transport.TransportServer
import java.io.IOException

/**
 * Foreground service that owns the camera, the frame pipeline and the host
 * transport. It outlives [MainActivity] and keeps streaming with the screen
 * off; the wake lock is held only while a capture session is active.
 *
 * Exported entry points are token protected: the host passes the same
 * [SessionToken] it later presents in `HOST_HELLO`. See `docs/android-app.md`.
 */
class CaptureService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null
    private var transport: TransportServer? = null
    private val logStreamer = LogStreamer()

    @Volatile
    private var running = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action
        val token = intent?.getStringExtra(SessionToken.EXTRA)
        if (token == null || !SessionToken.isValid(token)) {
            DeviceLog.w(TAG, "rejected action=$action: missing or malformed session token")
        } else {
            when (action) {
                ACTION_START -> startCapture(token)
                ACTION_STOP -> stopCapture("host_request")
                ACTION_STATUS -> DeviceLog.i(TAG, "status: running=$running session=${transport?.hasSession == true}")
                else -> DeviceLog.w(TAG, "rejected unknown action=$action")
            }
        }
        return START_NOT_STICKY
    }

    private fun startCapture(token: String) {
        when {
            running -> DeviceLog.i(TAG, "capture already running; ignoring duplicate start")
            !hasCameraPermission() -> {
                DeviceLog.e(TAG, "CAMERA permission not granted; provisioning must grant it before start")
                stopSelf()
            }
            !tryStartForeground(token) -> stopSelf()
            else -> {
                acquireWakeLock()
                running = true
                startTransport(token)
                DeviceLog.i(TAG, "capture service started (session token accepted)")
            }
        }
    }

    private fun hasCameraPermission(): Boolean =
        ContextCompat.checkSelfPermission(this, android.Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED

    private fun tryStartForeground(token: String): Boolean {
        createNotificationChannel()
        val notification = buildNotification(token)
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA,
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
            true
        } catch (e: SecurityException) {
            DeviceLog.e(TAG, "startForeground denied: ${e.message}", e)
            false
        } catch (e: IllegalStateException) {
            DeviceLog.e(TAG, "startForeground not allowed: ${e.message}", e)
            false
        }
    }

    private fun stopCapture(reason: String) {
        if (running) {
            DeviceLog.i(TAG, "stopping capture: $reason")
            running = false
            stopTransport()
            releaseWakeLock()
            stopForeground(STOP_FOREGROUND_REMOVE)
        }
        stopSelf()
    }

    private fun startTransport(token: String) {
        if (transport != null) {
            return
        }
        val server =
            TransportServer(
                sessionName = SOCKET_PREFIX + token,
                sessionToken = token,
                helloAck = { version -> HelloAckBuilder.build(this, version) },
                listener = transportListener,
            )
        try {
            server.start()
            transport = server
        } catch (e: IOException) {
            DeviceLog.e(TAG, "transport server failed to start: ${e.message}", e)
        }
    }

    private fun stopTransport() {
        logStreamer.detach()
        transport?.stop()
        transport = null
    }

    private val transportListener =
        object : TransportServer.Listener {
            override fun onSessionOpen(hostName: String?) {
                DeviceLog.i(TAG, "host session open: ${hostName ?: "unknown"}")
                transport?.let(logStreamer::attach)
            }

            override fun onSessionClosed(reason: String) {
                logStreamer.detach()
                DeviceLog.i(TAG, "host session closed: $reason")
            }

            override fun onPacket(packet: Packet) {
                if (packet.type == MessageType.COMMAND) {
                    // M4 implements control/capture commands; M3 only carries them.
                    DeviceLog.i(TAG, "COMMAND received (not implemented until M4)")
                } else {
                    DeviceLog.d(TAG, "unhandled packet ${packet.type}")
                }
            }
        }

    override fun onDestroy() {
        running = false
        stopTransport()
        releaseWakeLock()
        DeviceLog.i(TAG, "capture service destroyed")
        super.onDestroy()
    }

    private fun acquireWakeLock() {
        if (wakeLock == null) {
            val power = getSystemService(Context.POWER_SERVICE) as PowerManager
            wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, WAKE_LOCK_TAG)
            wakeLock?.setReferenceCounted(false)
        }
        // Held only between START and STOP; released in stopCapture/onDestroy.
        wakeLock?.acquire()
    }

    private fun releaseWakeLock() {
        wakeLock?.let { lock ->
            if (lock.isHeld) {
                lock.release()
            }
        }
    }

    private fun createNotificationChannel() {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val channel =
            NotificationChannel(
                CHANNEL_ID,
                getString(R.string.notification_channel_capture),
                NotificationManager.IMPORTANCE_LOW,
            )
        channel.description = getString(R.string.notification_channel_capture_description)
        channel.setShowBadge(false)
        manager.createNotificationChannel(channel)
    }

    private fun buildNotification(token: String): Notification {
        val open =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val stop =
            PendingIntent.getService(
                this,
                1,
                Intent(this, CaptureService::class.java)
                    .setAction(ACTION_STOP)
                    .putExtra(SessionToken.EXTRA, token),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        return NotificationCompat
            .Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_camera)
            .setContentTitle(getString(R.string.notification_capture_title))
            .setContentText(getString(R.string.notification_capture_text))
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .addAction(0, getString(R.string.notification_stop), stop)
            .build()
    }

    companion object {
        private const val TAG = "WatchdogCapture"
        private const val CHANNEL_ID = "watchdog-capture"
        private const val NOTIFICATION_ID = 1001
        private const val WAKE_LOCK_TAG = "watchdog:capture"
        private const val SOCKET_PREFIX = "watchdog-"

        const val ACTION_START = "app.watchdog.remote.action.START"
        const val ACTION_STOP = "app.watchdog.remote.action.STOP"
        const val ACTION_STATUS = "app.watchdog.remote.action.STATUS"
    }
}
