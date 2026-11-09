package app.watchdog.remote.protocol

import org.json.JSONArray
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

/** Parsed `COMMAND` payload (§5.5). */
data class CommandMessage(val id: String, val command: String, val params: JSONObject)

/** One control assignment inside `set_controls`. */
data class ControlAssignment(val id: String, val value: Any?)

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

    fun parseCommand(payload: ByteArray): CommandMessage =
        try {
            val json = JSONObject(text(payload))
            CommandMessage(
                id = json.getString("id"),
                command = json.getString("command"),
                params = json.optJSONObject("params") ?: JSONObject(),
            )
        } catch (e: JSONException) {
            throw ProtocolException(ProtocolError.MessageDecode, "invalid COMMAND: ${e.message}", e)
        }

    /**
     * Parse `params.controls` for `set_controls`, preserving wire order so
     * two assignments to the same id resolve last-writer-wins.
     */
    fun parseControlAssignments(params: JSONObject): List<ControlAssignment> {
        val array =
            params.optJSONArray("controls")
                ?: throw ProtocolException(
                    ProtocolError.MessageDecode,
                    "set_controls requires a controls array",
                )
        val out = ArrayList<ControlAssignment>(array.length())
        for (index in 0 until array.length()) {
            val item =
                array.optJSONObject(index)
                    ?: throw ProtocolException(
                        ProtocolError.MessageDecode,
                        "controls[$index] must be an object",
                    )
            val id =
                item.optString("id").takeIf { it.isNotEmpty() }
                    ?: throw ProtocolException(
                        ProtocolError.MessageDecode,
                        "controls[$index].id is required",
                    )
            out += ControlAssignment(id, if (item.has("value")) item.get("value") else null)
        }
        return out
    }

    /** One per-control outcome for `COMMAND_RESULT` (§5.6). */
    fun controlResult(
        control: String?,
        ok: Boolean,
        applied: Any? = null,
        errorCode: String? = null,
        message: String? = null,
    ): JSONObject {
        val json = JSONObject().put("ok", ok)
        if (control != null) {
            json.put("control", control)
        }
        if (applied != null) {
            json.put("applied", applied)
        }
        if (errorCode != null) {
            json.put("error_code", errorCode)
        }
        if (message != null) {
            json.put("message", message)
        }
        return json
    }

    fun encodeCommandResult(
        id: String,
        results: List<JSONObject>,
        timestampMs: Long = System.currentTimeMillis(),
        error: String? = null,
    ): ByteArray {
        val array = JSONArray()
        results.forEach { array.put(it) }
        val json =
            JSONObject()
                .put("id", id)
                .put("timestamp_ms", timestampMs)
                .put("results", array)
        if (error != null) {
            json.put("error", error)
        }
        return json.toString().toByteArray(Charsets.UTF_8)
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

    /** `EVENT` payload (§5.7): async app → host notification. */
    fun encodeEvent(
        kind: String,
        timestampMs: Long = System.currentTimeMillis(),
        data: JSONObject? = null,
    ): ByteArray {
        val json = JSONObject().put("kind", kind).put("timestamp_ms", timestampMs)
        if (data != null) {
            json.put("data", data)
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
