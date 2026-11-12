package app.watchdog.remote

import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/** Constants of the WCAG 2.1 relative-luminance and contrast formula. */
private const val WHITE = 255.0
private const val LUMINANCE_RED = 0.2126
private const val LUMINANCE_GREEN = 0.7152
private const val LUMINANCE_BLUE = 0.0722
private const val CONTRAST_OFFSET = 0.05
private const val LINEAR_THRESHOLD = 0.03928
private const val LINEAR_DIVISOR = 12.92
private const val GAMMA_OFFSET = 0.055
private const val GAMMA_SCALE = 1.055
private const val GAMMA = 2.4
private const val RED_SHIFT = 16
private const val GREEN_SHIFT = 8
private const val CHANNEL_MASK = 0xFF

/**
 * WCAG 2.1 relative-luminance contrast used by the on-device panel tests.
 * 4.5:1 is AA for normal text; 7:1 is AAA.
 */
internal object Contrast {
    fun ratio(foreground: Int, background: Int): Double {
        val first = luminance(foreground)
        val second = luminance(background)
        return (max(first, second) + CONTRAST_OFFSET) / (min(first, second) + CONTRAST_OFFSET)
    }

    private fun luminance(color: Int): Double {
        val red = linear((color shr RED_SHIFT) and CHANNEL_MASK)
        val green = linear((color shr GREEN_SHIFT) and CHANNEL_MASK)
        val blue = linear(color and CHANNEL_MASK)
        return LUMINANCE_RED * red + LUMINANCE_GREEN * green + LUMINANCE_BLUE * blue
    }

    private fun linear(channel: Int): Double {
        val value = channel / WHITE
        return if (value <= LINEAR_THRESHOLD) {
            value / LINEAR_DIVISOR
        } else {
            ((value + GAMMA_OFFSET) / GAMMA_SCALE).pow(GAMMA)
        }
    }
}

/** Palette values mirrored in `colors.xml` and verified by the panel tests. */
internal object PanelPalette {
    val normalBackground = 0xFF0E1216.toInt()
    val normalPanel = 0xFF1A2128.toInt()
    val normalTextPrimary = 0xFFF2F5F7.toInt()
    val normalTextSecondary = 0xFF9AA7B4.toInt()
    val normalAccent = 0xFF4FC3F7.toInt()

    val highContrastBackground = 0xFF000000.toInt()
    val highContrastPanel = 0xFF000000.toInt()
    val highContrastTextPrimary = 0xFFFFFFFF.toInt()
    val highContrastTextSecondary = 0xFFFFFF00.toInt()
    val highContrastAccent = 0xFF00FFFF.toInt()
}
