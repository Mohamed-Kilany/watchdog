package app.watchdog.remote.protocol

import org.json.JSONException
import org.json.JSONObject

/** Parsed `HOST_HELLO` payload (`docs/protocol.md` §5.1). */
data class HostHello(
    val protocolMin: Int,
    val protocolMax: Int,
    val hostName: String?,
    val hostVersion: String?,
    val sessionToken: String?,
)

/** Parsed `PING` payload (§5.11). */
data class PingMessage(val tsMs: Long, val nonce: String)

/** Parsed `BYE` payload (§5.12). */
data class ByeMessage(val reason: String, val detail: String?)

/** JSON payload codecs for the message types the app currently consumes. */
object JsonMessages {
    fun parseHostHello(payload: ByteArray): HostHello =
        try {
            val json = JSONObject(text(payload))
            val host = json.optJSONObject("host")
            HostHello(
                protocolMin = json.getInt("protocol_min"),
                protocolMax = json.getInt("protocol_max"),
                hostName = host?.optString("name"),
                hostVersion = host?.optString("version"),
                sessionToken = json.optString("session_token").takeIf { it.isNotEmpty() },
            )
        } catch (e: JSONException) {
            throw ProtocolException(ProtocolError.MessageDecode, "invalid HOST_HELLO: ${e.message}", e)
        }

    fun parsePing(payload: ByteArray): PingMessage =
        try {
            val json = JSONObject(text(payload))
            PingMessage(tsMs = json.getLong("ts_ms"), nonce = json.getString("nonce"))
        } catch (e: JSONException) {
            throw ProtocolException(ProtocolError.MessageDecode, "invalid PING: ${e.message}", e)
        }

    fun parseBye(payload: ByteArray): ByeMessage =
        try {
            val json = JSONObject(text(payload))
            ByeMessage(
                reason = json.optString("reason"),
                detail = json.optString("detail").takeIf { it.isNotEmpty() },
            )
        } catch (e: JSONException) {
            throw ProtocolException(ProtocolError.MessageDecode, "invalid BYE: ${e.message}", e)
        }

    fun encodePong(tsMs: Long, nonce: String, appTsNs: Long): ByteArray =
        JSONObject()
            .put("ts_ms", tsMs)
            .put("nonce", nonce)
            .put("app_ts_ns", appTsNs)
            .toString()
            .toByteArray(Charsets.UTF_8)

    fun encodeError(code: String, message: String, hint: String? = null): ByteArray {
        val json = JSONObject().put("code", code).put("message", message)
        if (hint != null) {
            json.put("hint", hint)
        }
        return json.toString().toByteArray(Charsets.UTF_8)
    }

    fun encodeBye(reason: String, detail: String? = null): ByteArray {
        val json = JSONObject().put("reason", reason)
        if (detail != null) {
            json.put("detail", detail)
        }
        return json.toString().toByteArray(Charsets.UTF_8)
    }

    private fun text(payload: ByteArray): String = String(payload, Charsets.UTF_8)
}
