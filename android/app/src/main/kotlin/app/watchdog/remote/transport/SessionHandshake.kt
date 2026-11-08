package app.watchdog.remote.transport

import app.watchdog.remote.protocol.JsonMessages
import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.PROTOCOL_VERSION
import app.watchdog.remote.protocol.Packet
import app.watchdog.remote.protocol.ProtocolException
import java.security.MessageDigest

/** Result of evaluating the host's first frame. */
sealed interface HandshakeDecision {
    data class Accepted(val protocolVersion: Int) : HandshakeDecision

    data class Rejected(val errorCode: String, val message: String, val hint: String? = null) : HandshakeDecision
}

/**
 * Pure handshake policy (`docs/protocol.md` §3): `HOST_HELLO` must be the first
 * frame, carry the provisioned session token and overlap our protocol range.
 */
object SessionHandshake {
    const val APP_MIN_VERSION = PROTOCOL_VERSION
    const val APP_MAX_VERSION = PROTOCOL_VERSION

    fun evaluate(packet: Packet, expectedToken: String): HandshakeDecision {
        if (packet.type != MessageType.HOST_HELLO) {
            return HandshakeDecision.Rejected(
                errorCode = "PROTOCOL_ERROR",
                message = "HOST_HELLO must be the first frame, got ${packet.type}",
            )
        }
        val hello =
            try {
                JsonMessages.parseHostHello(packet.payload)
            } catch (e: ProtocolException) {
                return HandshakeDecision.Rejected(
                    errorCode = "INVALID_REQUEST",
                    message = e.message ?: "invalid HOST_HELLO",
                )
            }
        if (!tokensMatch(hello.sessionToken, expectedToken)) {
            return HandshakeDecision.Rejected(
                errorCode = "UNAUTHORIZED",
                message = "session token mismatch",
            )
        }
        val version = minOf(hello.protocolMax, APP_MAX_VERSION)
        if (version < hello.protocolMin || version < APP_MIN_VERSION) {
            return HandshakeDecision.Rejected(
                errorCode = "PROTOCOL_VERSION_MISMATCH",
                message =
                    "no common protocol version (host ${hello.protocolMin}..${hello.protocolMax}, " +
                        "app $APP_MIN_VERSION..$APP_MAX_VERSION)",
                hint = "upgrade watchdog on the host",
            )
        }
        return HandshakeDecision.Accepted(version)
    }

    private fun tokensMatch(actual: String?, expected: String): Boolean {
        if (actual == null) {
            return false
        }
        return MessageDigest.isEqual(
            actual.toByteArray(Charsets.UTF_8),
            expected.toByteArray(Charsets.UTF_8),
        )
    }
}
