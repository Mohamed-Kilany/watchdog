package app.watchdog.remote

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.Settings
import android.view.Surface
import android.view.SurfaceHolder
import android.view.View
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.watchdog.remote.databinding.ActivityMainBinding
import kotlin.math.roundToInt

/**
 * On-device status UI (M3-T08): a live [SurfaceView] preview in the centre and
 * connection/camera panels on the sides. The preview is attached to the
 * running [CaptureService] through its local binder, so the exact capture
 * session the host streams from is also shown on the phone. Panels refresh
 * twice per second from [CaptureService.uiStatus]. When provisioning left a
 * runtime permission ungranted, the panels are replaced by a manual grant
 * screen; the normal layout is never shown half-provisioned.
 */
class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding
    private lateinit var controlPanel: UiControlPanel
    private var service: CaptureService? = null
    private var bound = false
    private var highContrast = false
    private var previewSurface: Surface? = null
    private val statusHandler = Handler(Looper.getMainLooper())

    private val statusTick =
        object : Runnable {
            override fun run() {
                refreshStatus()
                statusHandler.postDelayed(this, STATUS_INTERVAL_MS)
            }
        }

    private val serviceConnection =
        object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                service = (binder as? CaptureService.LocalBinder)?.service()
                previewSurface?.let { service?.setPreviewSurface(it) }
                refreshStatus()
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                service = null
                refreshStatus()
            }
        }

    private val surfaceCallback =
        object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) {
                holder.setFixedSize(PREVIEW_WIDTH, PREVIEW_HEIGHT)
                previewSurface = holder.surface
                service?.setPreviewSurface(holder.surface)
            }

            override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) = Unit

            override fun surfaceDestroyed(holder: SurfaceHolder) {
                previewSurface = null
                service?.setPreviewSurface(null)
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        highContrast =
            getSharedPreferences(UiPreferences.FILE, MODE_PRIVATE)
                .getBoolean(UiPreferences.HIGH_CONTRAST, false)
        setTheme(if (highContrast) R.style.Theme_WatchdogRemote_HighContrast else R.style.Theme_WatchdogRemote)
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        controlPanel = UiControlPanel(this, binding.leftControls, binding.rightControls)
        binding.version.text = getString(R.string.version_format, BuildConfig.VERSION_NAME)
        binding.highContrast.isChecked = highContrast
        binding.highContrast.setOnCheckedChangeListener { _, checked ->
            if (checked != highContrast) {
                highContrast = checked
                getSharedPreferences(UiPreferences.FILE, MODE_PRIVATE)
                    .edit()
                    .putBoolean(UiPreferences.HIGH_CONTRAST, checked)
                    .apply()
                recreate()
            }
        }
        binding.permissionSettings.setOnClickListener { openAppSettings() }
        binding.preview.holder.addCallback(surfaceCallback)
        binding.previewContainer.addOnLayoutChangeListener {
            _,
            left,
            top,
            right,
            bottom,
            oldLeft,
            oldTop,
            oldRight,
            oldBottom,
            ->
            if (right - left != oldRight - oldLeft || bottom - top != oldBottom - oldTop) {
                fitPreview()
            }
        }
        applyInsets()
    }

    override fun onStart() {
        super.onStart()
        bound =
            bindService(
                Intent(this, CaptureService::class.java),
                serviceConnection,
                Context.BIND_AUTO_CREATE,
            )
        statusHandler.post(statusTick)
    }

    override fun onStop() {
        statusHandler.removeCallbacks(statusTick)
        // Detach before the window (and its Surface) is torn down so the
        // service keeps streaming from the analysis surface alone.
        service?.setPreviewSurface(null)
        if (bound) {
            unbindService(serviceConnection)
            bound = false
        }
        service = null
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        refreshPermissionState()
        refreshStatus()
    }

    private fun refreshStatus() {
        val status = service?.uiStatus() ?: UiStatus()
        val placeholder = getString(R.string.value_placeholder)
        refreshConnection(status, placeholder)
        refreshCamera(status, placeholder)
        controlPanel.render(status.controls)
    }

    private fun refreshConnection(status: UiStatus, placeholder: String) {
        binding.connectionStatus.text =
            getString(if (status.hostConnected) R.string.value_connected else R.string.value_waiting)
        binding.hostName.text = status.hostName ?: getString(R.string.value_none)
        binding.sessionState.text =
            getString(if (status.hostConnected) R.string.value_active else R.string.value_idle)
        binding.fps.text = status.fps?.let { getString(R.string.value_fps_format, it) } ?: placeholder
        binding.resolution.text =
            if (status.width > 0 && status.height > 0) {
                getString(R.string.value_resolution_format, status.width, status.height)
            } else {
                placeholder
            }
        binding.jpegQuality.text =
            if (status.jpegQuality > 0) {
                getString(R.string.value_quality_format, status.jpegQuality)
            } else {
                placeholder
            }
        binding.droppedFrames.text = status.droppedFrames.toString()
    }

    private fun refreshCamera(status: UiStatus, placeholder: String) {
        binding.cameraId.text =
            status.cameraId?.let { id ->
                status.cameraFacing?.let { facing -> getString(R.string.value_camera_format, id, facing) } ?: id
            } ?: placeholder
        binding.cameraState.text = status.cameraStatus.name.lowercase()
        binding.storage.text =
            if (status.storageFreeMb > 0) {
                getString(R.string.value_storage_format, status.storageFreeMb)
            } else {
                placeholder
            }
        binding.battery.text =
            if (status.batteryPercent > 0) {
                getString(R.string.value_battery_format, status.batteryPercent)
            } else {
                placeholder
            }
        binding.temperature.text =
            status.temperatureC?.let { getString(R.string.value_temperature_format, it) } ?: placeholder
    }

    /**
     * Letterbox the preview to the 16:9 camera buffer instead of stretching
     * the buffer over the whole panel (which squashes the viewfinder).
     */
    private fun fitPreview() {
        val containerWidth = binding.previewContainer.width
        val containerHeight = binding.previewContainer.height
        if (containerWidth <= 0 || containerHeight <= 0) {
            return
        }
        val aspect = PREVIEW_WIDTH.toFloat() / PREVIEW_HEIGHT
        val containerAspect = containerWidth.toFloat() / containerHeight
        val width: Int
        val height: Int
        if (containerAspect > aspect) {
            height = containerHeight
            width = (height * aspect).roundToInt()
        } else {
            width = containerWidth
            height = (width / aspect).roundToInt()
        }
        val params = binding.preview.layoutParams
        if (params.width == width && params.height == height) {
            return
        }
        params.width = width
        params.height = height
        binding.preview.layoutParams = params
    }

    private fun applyInsets() {
        ViewCompat.setOnApplyWindowInsetsListener(binding.root) { view, insets ->
            val bars =
                insets.getInsets(
                    WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
                )
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            insets
        }
    }

    private fun refreshPermissionState() {
        val missing = missingPermissions()
        val granted = missing.isEmpty()
        binding.mainLayout.visibility = if (granted) View.VISIBLE else View.GONE
        binding.permissionLayout.visibility = if (granted) View.GONE else View.VISIBLE
        if (!granted) {
            val names = missing.joinToString { it.substringAfterLast('.') }
            binding.permissionMessage.text = getString(R.string.permission_message, names)
        }
    }

    private fun missingPermissions(): List<String> {
        val required =
            buildList {
                add(Manifest.permission.CAMERA)
                add(Manifest.permission.RECORD_AUDIO)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    add(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
        return required.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
    }

    private fun openAppSettings() {
        val intent =
            Intent(
                Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                Uri.fromParts("package", packageName, null),
            )
        startActivity(intent)
    }

    private companion object {
        const val STATUS_INTERVAL_MS = 500L
        const val PREVIEW_WIDTH = 1280
        const val PREVIEW_HEIGHT = 720
    }
}
