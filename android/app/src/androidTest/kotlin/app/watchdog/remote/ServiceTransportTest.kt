package app.watchdog.remote

import android.Manifest
import android.content.Context
import android.content.Intent
import android.net.LocalSocket
import android.net.LocalSocketAddress
import android.os.SystemClock
import androidx.core.content.ContextCompat
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.GrantPermissionRule
import app.watchdog.remote.protocol.FramePayload
import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.Packet
import app.watchdog.remote.protocol.PacketCodec
import app.watchdog.remote.protocol.StreamDecoder
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.Closeable
import java.io.IOException
import java.net.SocketTimeoutException

/**
 * M3-T11 end-to-end contract on the CI emulator: the foreground service opens
 * the (virtual scene) camera, serves the authenticated abstract socket and
 * streams JPEG frames; a second session replaces the first.
 */
@RunWith(AndroidJUnit4::class)
class ServiceTransportTest {
    @get:Rule
    val permissions: GrantPermissionRule =
        GrantPermissionRule.grant(
            Manifest.permission.CAMERA,
            Manifest.permission.RECORD_AUDIO,
            Manifest.permission.POST_NOTIFICATIONS,
        )

    private lateinit var context: Context
    private var scenario: ActivityScenario<MainActivity>? = null
    private val clients = mutableListOf<TestClient>()
    private val token = "0123456789abcdef0123456789abcdef"

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        // Starting a camera foreground service from the background is denied on
        // API 34+; bring the app to the foreground first, exactly like a user.
        scenario = ActivityScenario.launch(MainActivity::class.java)
        startService()
    }

    @After
    fun tearDown() {
        clients.forEach { it.close() }
        context.stopService(Intent(context, CaptureService::class.java))
        scenario?.close()
    }

    @Test
    fun handshakeAndFramesFlow() {
        val client = connect(token)
        client.sendHello(token)

        val ack = client.await(MessageType.HELLO_ACK)
        val helloAck = JSONObject(String(ack.payload, Charsets.UTF_8))
        assertEquals(1, helloAck.getInt("protocol_version"))
        assertEquals(context.packageName, helloAck.getJSONObject("app").getString("package"))

        val frame = FramePayload.decode(client.await(MessageType.FRAME, FRAME_TIMEOUT_MS).payload)
        assertTrue("JPEG payload must not be empty", frame.jpeg.isNotEmpty())
        assertTrue("frame must have real dimensions", frame.width > 0 && frame.height > 0)
    }

    @Test
    fun wrongTokenIsRejected() {
        val client = connect(token)
        client.sendHello("ffffffffffffffffffffffffffffffff")

        val error = JSONObject(String(client.await(MessageType.ERROR).payload, Charsets.UTF_8))
        assertEquals("UNAUTHORIZED", error.getString("code"))
        client.await(MessageType.BYE)
    }

    @Test
    fun secondSessionReplacesFirst() {
        val first = connect(token)
        first.sendHello(token)
        first.await(MessageType.HELLO_ACK)

        val second = connect(token)
        second.sendHello(token)
        second.await(MessageType.HELLO_ACK)

        val bye = JSONObject(String(first.await(MessageType.BYE).payload, Charsets.UTF_8))
        assertEquals("session_replaced", bye.getString("reason"))
    }

    private fun startService() {
        val intent =
            Intent(context, CaptureService::class.java)
                .setAction(CaptureService.ACTION_START)
                .putExtra(SessionToken.EXTRA, token)
        ContextCompat.startForegroundService(context, intent)
    }

    private fun connect(socketToken: String): TestClient {
        val deadline = SystemClock.elapsedRealtime() + CONNECT_TIMEOUT_MS
        var lastError: IOException? = null
        while (SystemClock.elapsedRealtime() < deadline) {
            val socket = LocalSocket()
            try {
                socket.connect(
                    LocalSocketAddress("watchdog-$socketToken", LocalSocketAddress.Namespace.ABSTRACT),
                )
                return TestClient(socket).also { clients += it }
            } catch (e: IOException) {
                lastError = e
                socket.close()
                SystemClock.sleep(RETRY_DELAY_MS)
            }
        }
        throw AssertionError("abstract socket watchdog-$socketToken never appeared", lastError)
    }

    private class TestClient(private val socket: LocalSocket) : Closeable {
        private val decoder = StreamDecoder()
        private val buffer = ByteArray(64 * 1024)
        private val pending = ArrayDeque<Packet>()

        fun sendHello(helloToken: String) {
            val hello =
                JSONObject()
                    .put("protocol_min", 1)
                    .put("protocol_max", 1)
                    .put("host", JSONObject().put("name", "ci-emulator").put("version", "test"))
                    .put("session_token", helloToken)
                    .toString()
                    .toByteArray(Charsets.UTF_8)
            socket.outputStream.write(PacketCodec.encode(Packet(MessageType.HOST_HELLO, 0, hello)))
            socket.outputStream.flush()
        }

        fun await(type: MessageType, timeoutMs: Long = DEFAULT_TIMEOUT_MS): Packet {
            val deadline = SystemClock.elapsedRealtime() + timeoutMs
            while (SystemClock.elapsedRealtime() < deadline) {
                val index = pending.indexOfFirst { it.type == type }
                if (index >= 0) {
                    return pending.removeAt(index)
                }
                socket.soTimeout = POLL_MS
                val read =
                    try {
                        socket.inputStream.read(buffer)
                    } catch (e: SocketTimeoutException) {
                        continue
                    } catch (e: IOException) {
                        // Android's LocalSocket reports SO_TIMEOUT as EAGAIN
                        // ("Try again") instead of SocketTimeoutException.
                        if (e.message == "Try again") {
                            continue
                        }
                        throw e
                    }
                if (read < 0) {
                    throw AssertionError("socket closed while waiting for $type")
                }
                pending.addAll(decoder.feed(buffer.copyOf(read)))
            }
            throw AssertionError("timed out waiting for $type")
        }

        override fun close() {
            socket.close()
        }
    }

    private companion object {
        const val CONNECT_TIMEOUT_MS = 15_000L
        const val FRAME_TIMEOUT_MS = 45_000L
        const val DEFAULT_TIMEOUT_MS = 10_000L
        const val RETRY_DELAY_MS = 100L
        const val POLL_MS = 500
    }
}
