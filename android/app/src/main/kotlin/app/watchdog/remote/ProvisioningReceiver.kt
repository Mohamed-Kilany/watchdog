package app.watchdog.remote

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.content.ContextCompat

/**
 * Token-protected exported entry point for provisioning.
 *
 * `adb shell am broadcast -n app.watchdog.remote/.ProvisioningReceiver \
 *   -a app.watchdog.remote.action.START --es <SessionToken.EXTRA> <hex>`
 *
 * The receiver validates the token and forwards to [CaptureService], which
 * validates it again. This keeps `am start-foreground-service` (direct) and
 * broadcast-based provisioning equally safe.
 */
class ProvisioningReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val action = intent.action
        val token = intent.getStringExtra(SessionToken.EXTRA)
        when {
            token == null || !SessionToken.isValid(token) ->
                Log.w(TAG, "rejected action=$action: missing or malformed session token")
            action != CaptureService.ACTION_START &&
                action != CaptureService.ACTION_STOP &&
                action != CaptureService.ACTION_STATUS ->
                Log.w(TAG, "rejected unknown action=$action")
            else -> forward(context, action, token)
        }
    }

    private fun forward(context: Context, action: String?, token: String) {
        val forward =
            Intent(context, CaptureService::class.java)
                .setAction(action)
                .putExtra(SessionToken.EXTRA, token)
        if (action == CaptureService.ACTION_START) {
            ContextCompat.startForegroundService(context, forward)
        } else {
            context.startService(forward)
        }
    }

    companion object {
        private const val TAG = "WatchdogProvisioning"
    }
}
