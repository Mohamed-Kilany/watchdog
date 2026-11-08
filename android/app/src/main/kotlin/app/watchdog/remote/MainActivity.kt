package app.watchdog.remote

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.View
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.watchdog.remote.databinding.ActivityMainBinding

/**
 * On-device status UI (M3-T08): live preview in the centre, connection panel
 * on the left, camera/device panel on the right. When provisioning left a
 * runtime permission ungranted, the panels are replaced by a manual grant
 * screen; the normal layout is never shown half-provisioned.
 */
class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        binding.version.text = getString(R.string.version_format, BuildConfig.VERSION_NAME)
        binding.permissionSettings.setOnClickListener { openAppSettings() }
        applyInsets()
    }

    override fun onResume() {
        super.onResume()
        refreshPermissionState()
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
}
