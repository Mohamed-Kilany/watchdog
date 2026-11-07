package app.watchdog.remote

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import app.watchdog.remote.databinding.ActivityMainBinding

/**
 * Placeholder MVP activity. M3-T08 replaces the body with the landscape
 * preview + side panels; M3-T03 moves camera ownership to the foreground
 * service.
 */
class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        binding.version.text = getString(R.string.version_format, BuildConfig.VERSION_NAME)
    }
}
