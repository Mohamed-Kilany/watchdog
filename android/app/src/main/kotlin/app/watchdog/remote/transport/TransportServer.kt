package app.watchdog.remote.transport

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.os.SystemClock
import app.watchdog.remote.log.DeviceLog
import app.watchdog.remote.protocol.JsonMessages
import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.Packet
import app.watchdog.remote.protocol.PacketCodec
import app.watchdog.remote.protocol.ProtocolError
import app.watchdog.remote.protocol.ProtocolException
import app.watchdog.remote.protocol.StreamDecoder
import java.io.IOException
import java.net.SocketTimeoutException
import java.util.ArrayDeque
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Abstract local-socket server (`localabstract:<sessionName>`) implementing the
 * app side of `docs/protocol.md`: one authenticated host session, framed
 * messages, PING→PONG heartbeat, bounded send queue (FRAME drop on overflow)
 * and takeover by a new authenticated session.
 */
class TransportServer(
    private val sessionName: String,
    private val sessionToken: String,
    private val helloAck: (Int) -> ByteArray,
    private val listener: Listener,
    private val boottimeNs: () -> Long = { SystemClock.elapsedRealtimeNanos() },
) {
    /** Session lifecycle callbacks (invoked from session threads). */
    interface Listener {
        fun onSessionOpen(hostName: String?)

        fun onSessionClosed(reason: String)

        fun onPacket(packet: Packet)
    }

    private val running = AtomicBoolean(false)
    private val lock = Any()
    private val sessions = mutableSetOf<Session>()
    private var serverSocket: LocalServerSocket? = null
    private var acceptThread: Thread? = null

    /** True while at least one authenticated host session is active. */
    val hasSession: Boolean
        get() = synchronized(lock) { sessions.any { it.authenticated } }

    fun start() {
        if (!running.compareAndSet(false, true)) {
            return
        }
        val socket = LocalServerSocket(sessionName)
        serverSocket = socket
        acceptThread =
            Thread({ acceptLoop(socket) }, "watchdog-accept").apply {
                isDaemon = true
                start()
            }
        DeviceLog.i(TAG, "listening on localabstract:$sessionName")
    }

    fun stop() {
        if (!running.compareAndSet(true, false)) {
            return
        }
        try {
            serverSocket?.close()
        } catch (e: IOException) {
            DeviceLog.d(TAG, "server socket close: ${e.message}")
        }
        serverSocket = null
        val current = synchronized(lock) { sessions.toList() }
        current.forEach { it.shutdown("app_shutdown") }
        acceptThread?.join(ACCEPT_JOIN_MS)
        acceptThread = null
        DeviceLog.i(TAG, "stopped")
    }

    /**
     * Queues a packet for the active session. `FRAME` and `LOG_CHUNK` packets
     * are dropped when the send queue is full; control packets fail the
     * session on overflow.
     */
    fun send(packet: Packet): Boolean {
        val session = synchronized(lock) { sessions.firstOrNull { it.authenticated && !it.isClosed } }
        return session?.send(packet) ?: false
    }

    /** Best-effort `LOG_CHUNK` send; logs are lossy by design. */
    fun sendLog(payload: ByteArray): Boolean {
        val session = synchronized(lock) { sessions.firstOrNull { it.authenticated && !it.isClosed } }
        return session?.sendLog(payload) ?: false
    }

    private fun acceptLoop(socket: LocalServerSocket) {
        while (running.get()) {
            val client =
                try {
                    socket.accept()
                } catch (e: IOException) {
                    if (running.get()) {
                        DeviceLog.w(TAG, "accept failed: ${e.message}")
                    }
                    break
                }
            val session = Session(client)
            Thread({ session.run() }, "watchdog-session").apply {
                isDaemon = true
                start()
            }
        }
    }

    private fun register(session: Session) {
        val evicted =
            synchronized(lock) {
                val old = sessions.filter { it !== session && it.authenticated }
                sessions += session
                old
            }
        evicted.forEach { it.shutdown("session_replaced") }
    }

    private fun unregister(session: Session) {
        synchronized(lock) { sessions.remove(session) }
    }

    private inner class Session(private val socket: LocalSocket) {
        private val peer: Peer = SocketPeer(socket)
        private val decoder = StreamDecoder()
        private val pending = ArrayDeque<Packet>()
        private val outbound = OutboundQueue(SEND_QUEUE_CAPACITY)
        private val closed = AtomicBoolean(false)

        @Volatile
        private var writer: Thread? = null

        @Volatile
        private var byeSent = false

        @Volatile
        var authenticated = false
            private set

        @Volatile
        private var hostName: String? = null

        val isClosed: Boolean
            get() = closed.get()

        fun run() {
            var reason = "error"
            try {
                peer.setReadTimeout(HANDSHAKE_TIMEOUT_MS)
                if (!handshake()) {
                    return
                }
                if (!running.get()) {
                    reason = "app_shutdown"
                    return
                }
                authenticated = true
                register(this)
                startWriter()
                listener.onSessionOpen(hostName)
                reason = readLoop()
            } catch (e: ProtocolException) {
                DeviceLog.w(TAG, "protocol error: ${e.message}")
                sendError(PROTOCOL_ERROR_CODE, e.message ?: "protocol error")
                reason = "protocol_error"
            } catch (e: IOException) {
                DeviceLog.i(TAG, "session I/O ended: ${e.message}")
                reason = "io_error"
            } finally {
                shutdown(reason)
                unregister(this)
            }
        }

        fun send(packet: Packet): Boolean {
            if (closed.get()) {
                return false
            }
            val accepted = outbound.offer(packet)
            if (!accepted && packet.type != MessageType.FRAME && packet.type != MessageType.LOG_CHUNK) {
                DeviceLog.w(TAG, "control send queue overflow; closing session")
                shutdown("send_queue_overflow")
            }
            return accepted
        }

        fun sendLog(payload: ByteArray): Boolean {
            if (closed.get()) {
                return false
            }
            return outbound.offer(Packet(MessageType.LOG_CHUNK, 0, payload))
        }

        fun shutdown(reason: String) {
            if (!closed.compareAndSet(false, true)) {
                return
            }
            outbound.close()
            if (authenticated && !byeSent) {
                sendBye(byeReason(reason))
            }
            peer.close()
            writer?.interrupt()
            writer = null
            unregister(this)
            if (authenticated) {
                listener.onSessionClosed(reason)
            }
        }

        private fun handshake(): Boolean {
            val buffer = ByteArray(READ_BUFFER)
            while (running.get()) {
                val read =
                    try {
                        peer.read(buffer)
                    } catch (e: SocketTimeoutException) {
                        DeviceLog.w(TAG, "handshake timed out", e)
                        return false
                    }
                if (read < 0) {
                    return false
                }
                val packets = decoder.feed(buffer.copyOf(read))
                if (packets.isEmpty()) {
                    continue
                }
                pending.addAll(packets.drop(1))
                when (val decision = SessionHandshake.evaluate(packets.first(), sessionToken)) {
                    is HandshakeDecision.Accepted -> {
                        val hello = JsonMessages.parseHostHello(packets.first().payload)
                        val ack = Packet(MessageType.HELLO_ACK, 0, helloAck(decision.protocolVersion))
                        peer.write(PacketCodec.encode(ack))
                        peer.setReadTimeout(HEARTBEAT_TIMEOUT_MS)
                        hostName = hello.hostName
                        DeviceLog.i(TAG, "host session accepted (${hello.hostName ?: "unknown host"})")
                        return true
                    }
                    is HandshakeDecision.Rejected -> {
                        sendError(decision.errorCode, decision.message, decision.hint)
                        sendBye("error", decision.errorCode)
                        DeviceLog.w(TAG, "handshake rejected: ${decision.errorCode}")
                        return false
                    }
                }
            }
            return false
        }

        private fun readLoop(): String {
            val buffer = ByteArray(READ_BUFFER)
            while (running.get() && !closed.get()) {
                while (pending.isNotEmpty()) {
                    val reason = handle(pending.removeFirst())
                    if (reason != null) {
                        return reason
                    }
                }
                val read =
                    try {
                        peer.read(buffer)
                    } catch (e: SocketTimeoutException) {
                        DeviceLog.w(TAG, "heartbeat timeout", e)
                        return "heartbeat_timeout"
                    }
                if (read < 0) {
                    return "host_closed"
                }
                for (packet in decoder.feed(buffer.copyOf(read))) {
                    val reason = handle(packet)
                    if (reason != null) {
                        return reason
                    }
                }
            }
            return "app_shutdown"
        }

        /** Returns a close reason when the packet terminates the session. */
        private fun handle(packet: Packet): String? =
            when (packet.type) {
                MessageType.PING -> {
                    val ping = JsonMessages.parsePing(packet.payload)
                    val pong = JsonMessages.encodePong(ping.tsMs, ping.nonce, boottimeNs())
                    peer.write(PacketCodec.encode(Packet(MessageType.PONG, 0, pong)))
                    null
                }
                MessageType.BYE -> {
                    val bye = JsonMessages.parseBye(packet.payload)
                    byeSent = true
                    bye.reason.ifEmpty { "host_bye" }
                }
                MessageType.HOST_HELLO ->
                    throw ProtocolException(ProtocolError.MessageDecode, "duplicate HOST_HELLO")
                else -> {
                    listener.onPacket(packet)
                    null
                }
            }

        private fun startWriter() {
            writer =
                Thread({ writerLoop() }, "watchdog-writer").apply {
                    isDaemon = true
                    start()
                }
        }

        private fun writerLoop() {
            while (running.get() && !closed.get()) {
                val packet = outbound.poll(WRITER_POLL_MS) ?: continue
                try {
                    peer.write(PacketCodec.encode(packet))
                } catch (e: IOException) {
                    DeviceLog.w(TAG, "write failed: ${e.message}")
                    shutdown("send_failed")
                    return
                }
            }
        }

        @Synchronized
        private fun sendBye(reason: String, detail: String? = null) {
            if (byeSent) {
                return
            }
            byeSent = true
            sendBestEffort(Packet(MessageType.BYE, 0, JsonMessages.encodeBye(reason, detail)))
        }

        private fun sendError(code: String, message: String, hint: String? = null) {
            sendBestEffort(Packet(MessageType.ERROR, 0, JsonMessages.encodeError(code, message, hint)))
        }

        private fun sendBestEffort(packet: Packet) {
            try {
                peer.write(PacketCodec.encode(packet))
            } catch (e: IOException) {
                DeviceLog.d(TAG, "best-effort send failed: ${e.message}")
            }
        }
    }

    private companion object {
        const val TAG = "WatchdogTransport"
        const val SEND_QUEUE_CAPACITY = 8
        const val READ_BUFFER = 16 * 1024
        const val WRITER_POLL_MS = 100L
        const val HANDSHAKE_TIMEOUT_MS = 10_000
        const val HEARTBEAT_TIMEOUT_MS = 15_000
        const val ACCEPT_JOIN_MS = 1_000L
        const val PROTOCOL_ERROR_CODE = "PROTOCOL_ERROR"

        fun byeReason(reason: String): String =
            when (reason) {
                "session_replaced" -> "session_replaced"
                "app_shutdown" -> "app_shutdown"
                else -> "error"
            }
    }
}
