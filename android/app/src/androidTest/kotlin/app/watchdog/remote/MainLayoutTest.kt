package app.watchdog.remote

import android.Manifest
import android.graphics.Rect
import android.view.View
import androidx.test.ext.junit.rules.ActivityScenarioRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.GrantPermissionRule
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * M3-T08 layout contract: the side panels never overlap the video area and
 * the manual permission screen only replaces the panels when a permission is
 * actually missing. Runs on the CI emulator (M3-T11).
 */
@RunWith(AndroidJUnit4::class)
class MainLayoutTest {
    @get:Rule
    val permissions: GrantPermissionRule =
        GrantPermissionRule.grant(
            Manifest.permission.CAMERA,
            Manifest.permission.RECORD_AUDIO,
            Manifest.permission.POST_NOTIFICATIONS,
        )

    @get:Rule
    val activity = ActivityScenarioRule(MainActivity::class.java)

    @Test
    fun panelsDoNotOverlapPreview() {
        activity.scenario.onActivity { current ->
            val preview = current.findViewById<View>(R.id.previewContainer)
            val left = current.findViewById<View>(R.id.leftPanel)
            val right = current.findViewById<View>(R.id.rightPanel)

            assertTrue("preview must be laid out", preview.width > 0 && preview.height > 0)
            assertFalse("left panel overlaps preview", bounds(left).overlaps(bounds(preview)))
            assertFalse("right panel overlaps preview", bounds(right).overlaps(bounds(preview)))
            assertTrue("left panel must be left of preview", left.right <= preview.left)
            assertTrue("right panel must be right of preview", preview.right <= right.left)
        }
    }

    @Test
    fun mainLayoutIsVisibleWhenPermissionsAreGranted() {
        activity.scenario.onActivity { current ->
            val main = current.findViewById<View>(R.id.mainLayout)
            val permission = current.findViewById<View>(R.id.permissionLayout)
            assertTrue("main layout should be visible", main.visibility == View.VISIBLE)
            assertTrue("permission screen should be hidden", permission.visibility == View.GONE)
        }
    }

    private fun bounds(view: View): Rect {
        val location = IntArray(2)
        view.getLocationOnScreen(location)
        return Rect(location[0], location[1], location[0] + view.width, location[1] + view.height)
    }
}
