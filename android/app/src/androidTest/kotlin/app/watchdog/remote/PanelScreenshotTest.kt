package app.watchdog.remote

import android.Manifest
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.widget.CheckBox
import android.widget.TextView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * M4-T13 screenshot tests: render the real activity (no camera required),
 * assert the panel backgrounds and text contrast, and save PNGs for review.
 * Runs in the manual emulator workflow; the physical-device check is recorded
 * in `docs/verification/`.
 */
@RunWith(AndroidJUnit4::class)
class PanelScreenshotTest {
    @get:Rule
    val permissions: GrantPermissionRule =
        GrantPermissionRule.grant(
            Manifest.permission.CAMERA,
            Manifest.permission.RECORD_AUDIO,
            Manifest.permission.POST_NOTIFICATIONS,
        )

    private val context: Context
        get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Before
    fun useNormalMode() {
        setHighContrast(false)
    }

    @After
    fun restoreNormalMode() {
        setHighContrast(false)
    }

    @Test
    fun normalModeRendersReadablePanels() {
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        try {
            withLaidOutActivity(scenario) { activity ->
                assertEquals(PanelPalette.normalPanel, themeColor(activity, R.attr.watchdogPanel))
                assertTextContrast(activity, PanelPalette.normalPanel, MINIMUM_AA)
                assertPanelPixels(activity, capture(activity, "panels-normal.png"), PanelPalette.normalPanel)
            }
        } finally {
            scenario.close()
        }
    }

    @Test
    fun highContrastModeRendersReadablePanels() {
        setHighContrast(true)
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        try {
            withLaidOutActivity(scenario) { activity ->
                assertTrue(
                    "high-contrast toggle should be on",
                    activity.findViewById<CheckBox>(R.id.highContrast).isChecked,
                )
                assertEquals(
                    PanelPalette.highContrastPanel,
                    themeColor(activity, R.attr.watchdogPanel),
                )
                assertTextContrast(activity, PanelPalette.highContrastPanel, MINIMUM_AAA)
                assertPanelPixels(
                    activity,
                    capture(activity, "panels-high-contrast.png"),
                    PanelPalette.highContrastPanel,
                )
            }
        } finally {
            scenario.close()
        }
    }

    private fun setHighContrast(enabled: Boolean) {
        context
            .getSharedPreferences(UiPreferences.FILE, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(UiPreferences.HIGH_CONTRAST, enabled)
            .commit()
    }

    private fun themeColor(activity: MainActivity, attribute: Int): Int {
        val values = activity.obtainStyledAttributes(intArrayOf(attribute))
        return try {
            values.getColor(0, Color.TRANSPARENT)
        } finally {
            values.recycle()
        }
    }

    private fun assertTextContrast(activity: MainActivity, panel: Int, minimum: Double) {
        val textViews = ArrayList<TextView>()
        collectTextViews(activity.findViewById(R.id.leftPanel), textViews)
        collectTextViews(activity.findViewById(R.id.rightPanel), textViews)
        assertTrue("panels should contain text", textViews.isNotEmpty())
        for (textView in textViews) {
            if (!textView.isShown || textView.text.isNullOrBlank()) {
                continue
            }
            val ratio = Contrast.ratio(textView.currentTextColor, panel)
            assertTrue(
                "'${textView.text}' contrast $ratio is below $minimum",
                ratio >= minimum,
            )
        }
    }

    private fun collectTextViews(view: View, out: MutableList<TextView>) {
        if (view is TextView) {
            out += view
        }
        if (view is ViewGroup) {
            for (index in 0 until view.childCount) {
                collectTextViews(view.getChildAt(index), out)
            }
        }
    }

    private fun assertPanelPixels(activity: MainActivity, bitmap: Bitmap, panel: Int) {
        val root = activity.window.decorView
        val rootLocation = IntArray(2)
        root.getLocationOnScreen(rootLocation)
        val left = activity.findViewById<View>(R.id.leftPanel)
        val leftLocation = IntArray(2)
        left.getLocationOnScreen(leftLocation)
        val leftX = leftLocation[0] - rootLocation[0]
        val leftY = leftLocation[1] - rootLocation[1]
        assertEquals("left panel background", panel, bitmap.getPixel(leftX + 2, leftY + 2))

        val preview = activity.findViewById<View>(R.id.previewContainer)
        val previewLocation = IntArray(2)
        preview.getLocationOnScreen(previewLocation)
        val previewX = previewLocation[0] - rootLocation[0] + preview.width / 2
        val previewY = previewLocation[1] - rootLocation[1] + preview.height / 2
        assertEquals("preview background", Color.BLACK, bitmap.getPixel(previewX, previewY))

        var textPixels = 0
        for (y in leftY until leftY + left.height step PIXEL_STEP) {
            for (x in leftX until leftX + left.width step PIXEL_STEP) {
                if (bitmap.getPixel(x, y) != panel) {
                    textPixels++
                }
            }
        }
        assertTrue("left panel should render text, found $textPixels pixels", textPixels > MINIMUM_TEXT_PIXELS)
    }

    private fun capture(activity: MainActivity, name: String): Bitmap {
        val root = activity.window.decorView
        val bitmap = Bitmap.createBitmap(root.width, root.height, Bitmap.Config.ARGB_8888)
        root.draw(Canvas(bitmap))
        val directory =
            activity.getExternalFilesDir(SCREENSHOT_DIR)
                ?: File(activity.filesDir, SCREENSHOT_DIR)
        directory.mkdirs()
        val file = File(directory, name)
        FileOutputStream(file).use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        Log.i(TAG, "screenshot: ${file.absolutePath}")
        return bitmap
    }

    private fun withLaidOutActivity(scenario: ActivityScenario<MainActivity>, block: (MainActivity) -> Unit) {
        val latch = CountDownLatch(1)
        var failure: Throwable? = null
        scenario.onActivity { activity ->
            activity.window.decorView.post {
                try {
                    block(activity)
                } catch (error: Throwable) {
                    failure = error
                } finally {
                    latch.countDown()
                }
            }
        }
        assertTrue("activity did not finish layout", latch.await(LAYOUT_TIMEOUT_SECONDS, TimeUnit.SECONDS))
        failure?.let { throw AssertionError(it) }
    }

    private companion object {
        const val TAG = "PanelScreenshotTest"
        const val SCREENSHOT_DIR = "screenshots"
        const val MINIMUM_AA = 4.5
        const val MINIMUM_AAA = 7.0
        const val PIXEL_STEP = 2
        const val MINIMUM_TEXT_PIXELS = 100
        const val LAYOUT_TIMEOUT_SECONDS = 5L
    }
}
