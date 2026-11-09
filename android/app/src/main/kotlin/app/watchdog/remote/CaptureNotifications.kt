package app.watchdog.remote

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.PowerManager
import androidx.core.app.NotificationCompat

/**
 * Foreground-service notification and wake-lock plumbing, kept out of
 * [CaptureService] so the service stays focused on capture coordination.
 */
object CaptureNotifications {
    const val CHANNEL_ID = "watchdog-capture"
    const val NOTIFICATION_ID = 1001
    private const val WAKE_LOCK_TAG = "watchdog:capture"

    fun createChannel(service: CaptureService) {
        val manager = service.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val channel =
            NotificationChannel(
                CHANNEL_ID,
                service.getString(R.string.notification_channel_capture),
                NotificationManager.IMPORTANCE_LOW,
            )
        channel.description = service.getString(R.string.notification_channel_capture_description)
        channel.setShowBadge(false)
        manager.createNotificationChannel(channel)
    }

    fun build(service: CaptureService, token: String): Notification {
        val open =
            PendingIntent.getActivity(
                service,
                0,
                Intent(service, MainActivity::class.java),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val stop =
            PendingIntent.getService(
                service,
                1,
                Intent(service, CaptureService::class.java)
                    .setAction(CaptureService.ACTION_STOP)
                    .putExtra(SessionToken.EXTRA, token),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        return NotificationCompat
            .Builder(service, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_camera)
            .setContentTitle(service.getString(R.string.notification_capture_title))
            .setContentText(service.getString(R.string.notification_capture_text))
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .addAction(0, service.getString(R.string.notification_stop), stop)
            .build()
    }

    /** Reuse [existing] or create the partial wake lock, then acquire it. */
    fun acquire(context: Context, existing: PowerManager.WakeLock?): PowerManager.WakeLock {
        val lock =
            existing
                ?: (context.getSystemService(Context.POWER_SERVICE) as PowerManager)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, WAKE_LOCK_TAG)
                    .apply { setReferenceCounted(false) }
        // Held only between START and STOP; released in stopCapture/onDestroy.
        lock.acquire()
        return lock
    }

    fun release(lock: PowerManager.WakeLock?) {
        if (lock != null && lock.isHeld) {
            lock.release()
        }
    }
}
