package app.watchdog.remote.transport

import app.watchdog.remote.log.DeviceLog

/**
 * Bridges [DeviceLog] to one authenticated transport session: the backlog is
 * flushed on attach (so `watchdog device logs` shows what happened before the
 * host connected) and new records stream as `LOG_CHUNK` while attached.
 */
class LogStreamer {
    private var attached: TransportServer? = null

    fun attach(transport: TransportServer) {
        if (attached === transport) {
            return
        }
        attached = transport
        val backlog = DeviceLog.attach { payload -> transport.sendLog(payload) }
        backlog.forEach(transport::sendLog)
    }

    fun detach() {
        DeviceLog.detach()
        attached = null
    }
}
