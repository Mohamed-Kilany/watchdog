package app.watchdog.remote.transport

import android.net.LocalSocket
import java.io.IOException

/** Minimal blocking byte-stream abstraction over a local socket. */
interface Peer {
    /** Reads up to `buffer.size` bytes; returns -1 on EOF. */
    fun read(buffer: ByteArray): Int

    /** Writes all bytes; thread-safe. */
    fun write(bytes: ByteArray)

    fun setReadTimeout(timeoutMs: Int)

    fun close()
}

/** [Peer] backed by an accepted Android [LocalSocket] (abstract namespace). */
class SocketPeer(private val socket: LocalSocket) : Peer {
    private val input by lazy { socket.inputStream }
    private val output by lazy { socket.outputStream }
    private val writeLock = Any()

    override fun read(buffer: ByteArray): Int = input.read(buffer)

    override fun write(bytes: ByteArray) {
        synchronized(writeLock) {
            output.write(bytes)
            output.flush()
        }
    }

    override fun setReadTimeout(timeoutMs: Int) {
        socket.soTimeout = timeoutMs
    }

    override fun close() {
        try {
            socket.close()
        } catch (e: IOException) {
            // Closing an already-broken socket is not an error.
            android.util.Log.d("WatchdogPeer", "close: ${e.message}")
        }
    }
}
