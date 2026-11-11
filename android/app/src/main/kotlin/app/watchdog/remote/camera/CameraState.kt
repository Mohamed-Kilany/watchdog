package app.watchdog.remote.camera

/** Explicit camera lifecycle states (`docs/android-app.md`, M3-T04). */
enum class CameraStatus {
    CLOSED,
    OPENING,
    OPEN,
    CONFIGURED,
    STREAMING,
    RECOVERING,
    ERROR,
}

/** Inputs of the camera state machine. */
sealed interface CameraEvent {
    data class Open(val cameraId: String) : CameraEvent

    /** Switch to another camera while the service stays alive. */
    data class Switch(val cameraId: String) : CameraEvent

    data object Opened : CameraEvent

    data object Configured : CameraEvent

    data object StreamStarted : CameraEvent

    data object StreamStopped : CameraEvent

    data class Failed(val reason: String, val cameraInUse: Boolean = false) : CameraEvent

    data object Disconnected : CameraEvent

    data object Recover : CameraEvent

    data object Close : CameraEvent

    data object Closed : CameraEvent
}

/** Immutable view of the camera state, safe to publish to the UI and host. */
data class CameraSnapshot(
    val status: CameraStatus = CameraStatus.CLOSED,
    val cameraId: String? = null,
    val lastError: String? = null,
    val recoveries: Int = 0,
)

/**
 * Pure camera state machine. Keeping it free of Camera2 types makes every
 * transition unit-testable with fakes and fault-injection event sequences.
 *
 * Invalid events are ignored (the snapshot is returned unchanged) so late
 * callbacks from a closed device can never resurrect it.
 */
class CameraStateMachine(initial: CameraSnapshot = CameraSnapshot()) {
    @Volatile
    var snapshot: CameraSnapshot = initial
        private set

    fun onEvent(event: CameraEvent): CameraSnapshot {
        snapshot = reduce(snapshot, event)
        return snapshot
    }

    private fun reduce(state: CameraSnapshot, event: CameraEvent): CameraSnapshot =
        when (event) {
            is CameraEvent.Open -> onOpen(state, event.cameraId)
            is CameraEvent.Switch ->
                state.copy(status = CameraStatus.OPENING, cameraId = event.cameraId, lastError = null)
            is CameraEvent.Failed -> onFailed(state, event)
            CameraEvent.Disconnected -> onDisconnected(state)
            CameraEvent.Recover -> onRecover(state)
            CameraEvent.Close -> state.copy(status = CameraStatus.CLOSED, lastError = null)
            else -> onTransition(state, event)
        }

    private fun onOpen(state: CameraSnapshot, cameraId: String): CameraSnapshot =
        if (state.status in REOPENABLE) {
            state.copy(status = CameraStatus.OPENING, cameraId = cameraId, lastError = null)
        } else {
            state
        }

    private fun onFailed(state: CameraSnapshot, event: CameraEvent.Failed): CameraSnapshot =
        if (state.status == CameraStatus.CLOSED) {
            state
        } else {
            state.copy(status = CameraStatus.ERROR, lastError = event.reason)
        }

    private fun onDisconnected(state: CameraSnapshot): CameraSnapshot =
        if (state.status == CameraStatus.CLOSED || state.status == CameraStatus.ERROR) {
            state
        } else {
            state.copy(status = CameraStatus.RECOVERING, lastError = "camera disconnected")
        }

    private fun onRecover(state: CameraSnapshot): CameraSnapshot =
        if (state.status == CameraStatus.ERROR || state.status == CameraStatus.RECOVERING) {
            state.copy(status = CameraStatus.OPENING, recoveries = state.recoveries + 1)
        } else {
            state
        }

    private fun onTransition(state: CameraSnapshot, event: CameraEvent): CameraSnapshot =
        when (event) {
            CameraEvent.Opened ->
                if (state.status == CameraStatus.OPENING || state.status == CameraStatus.RECOVERING) {
                    state.copy(status = CameraStatus.OPEN)
                } else {
                    state
                }
            CameraEvent.Configured ->
                if (state.status == CameraStatus.OPEN) state.copy(status = CameraStatus.CONFIGURED) else state
            CameraEvent.StreamStarted ->
                if (state.status == CameraStatus.CONFIGURED || state.status == CameraStatus.STREAMING) {
                    state.copy(status = CameraStatus.STREAMING)
                } else {
                    state
                }
            CameraEvent.StreamStopped ->
                if (state.status == CameraStatus.STREAMING) state.copy(status = CameraStatus.CONFIGURED) else state
            // `onClosed` also fires after `onError`/`onDisconnected` closed the
            // failed device. A stale Closed must not erase ERROR/RECOVERING,
            // otherwise recovery runs while the machine is stuck at CLOSED.
            CameraEvent.Closed ->
                if (state.status in ACTIVE) state.copy(status = CameraStatus.CLOSED) else state
            else -> state
        }

    private companion object {
        private val REOPENABLE = setOf(CameraStatus.CLOSED, CameraStatus.ERROR, CameraStatus.RECOVERING)
        private val ACTIVE =
            setOf(
                CameraStatus.OPENING,
                CameraStatus.OPEN,
                CameraStatus.CONFIGURED,
                CameraStatus.STREAMING,
            )
    }
}
