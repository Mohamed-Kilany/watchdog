package app.watchdog.remote

import org.junit.Assert.assertTrue
import org.junit.Test

/** M4-T13: both panel palettes meet their accessibility contrast targets. */
class PanelContrastTest {
    @Test
    fun normalPaletteMeetsAaForNormalText() {
        assertContrast("primary", PanelPalette.normalTextPrimary, PanelPalette.normalPanel, 4.5)
        assertContrast("secondary", PanelPalette.normalTextSecondary, PanelPalette.normalPanel, 4.5)
        assertContrast("accent", PanelPalette.normalAccent, PanelPalette.normalPanel, 4.5)
    }

    @Test
    fun highContrastPaletteMeetsAaaForNormalText() {
        assertContrast("primary", PanelPalette.highContrastTextPrimary, PanelPalette.highContrastPanel, 7.0)
        assertContrast("secondary", PanelPalette.highContrastTextSecondary, PanelPalette.highContrastPanel, 7.0)
        assertContrast("accent", PanelPalette.highContrastAccent, PanelPalette.highContrastPanel, 7.0)
    }

    @Test
    fun highContrastPaletteIsStricterThanNormal() {
        assertTrue(
            Contrast.ratio(PanelPalette.highContrastTextSecondary, PanelPalette.highContrastPanel) >
                Contrast.ratio(PanelPalette.normalTextSecondary, PanelPalette.normalPanel),
        )
        assertTrue(
            Contrast.ratio(PanelPalette.highContrastAccent, PanelPalette.highContrastPanel) >
                Contrast.ratio(PanelPalette.normalAccent, PanelPalette.normalPanel),
        )
    }

    private fun assertContrast(name: String, foreground: Int, background: Int, minimum: Double) {
        val ratio = Contrast.ratio(foreground, background)
        assertTrue("$name contrast $ratio is below $minimum", ratio >= minimum)
    }
}
