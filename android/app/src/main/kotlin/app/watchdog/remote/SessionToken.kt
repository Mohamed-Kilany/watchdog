package app.watchdog.remote

import java.security.SecureRandom

/**
 * 128-bit session token shared by the host and the app.
 *
 * The host generates the token, passes it with the provisioning start intent
 * ([EXTRA]) and echoes it in `HOST_HELLO.session_token`. Exported entry points
 * reject calls without a well-formed token, and the transport server only
 * accepts a host that presents the same token.
 */
object SessionToken {
    /** Intent extra carrying the token (lowercase hex, 32 characters). */
    const val EXTRA = "app.watchdog.remote.extra.SESSION_TOKEN"

    private const val HEX_CHARS = 32
    private val HEX_PATTERN = Regex("^[0-9a-f]{$HEX_CHARS}$")
    private val random = SecureRandom()

    /** Returns a fresh random 128-bit token as lowercase hex. */
    fun generate(): String {
        val bytes = ByteArray(HEX_CHARS / 2)
        random.nextBytes(bytes)
        return bytes.joinToString(separator = "") { "%02x".format(it) }
    }

    /** True when [token] is exactly 32 lowercase hex characters. */
    fun isValid(token: String?): Boolean = token != null && HEX_PATTERN.matches(token)
}
