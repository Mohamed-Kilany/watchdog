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
            is CameraEvent.Open ->
                when (state.status) {
                    CameraStatus.CLOSED, CameraStatus.ERROR, CameraStatus.RECOVERING ->
                        state.copy(
                            status = CameraStatus.OPENING,
                            cameraId = event.cameraId,
                            lastError = null,
                        )
                    else -> state
                }
            CameraEvent.Opened ->
                when (state.status) {
                    CameraStatus.OPENING, CameraStatus.RECOVERING ->
                        state.copy(status = CameraStatus.OPEN)
                    else -> state
                }
            CameraEvent.Configured ->
                when (state.status) {
                    CameraStatus.OPEN -> state.copy(status = CameraStatus.CONFIGURED)
                    else -> state
                }
            CameraEvent.StreamStarted ->
                when (state.status) {
                    CameraStatus.CONFIGURED, CameraStatus.STREAMING ->
                        state.copy(status = CameraStatus.STREAMING)
                    else -> state
                }
            CameraEvent.StreamStopped ->
                when (state.status) {
                    CameraStatus.STREAMING -> state.copy(status = CameraStatus.CONFIGURED)
                    else -> state
                }
            is CameraEvent.Failed ->
                when (state.status) {
                    CameraStatus.CLOSED -> state
                    else -> state.copy(status = CameraStatus.ERROR, lastError = event.reason)
                }
            CameraEvent.Disconnected ->
                when (state.status) {
                    CameraStatus.CLOSED, CameraStatus.ERROR -> state
                    else -> state.copy(status = CameraStatus.RECOVERING, lastError = "camera disconnected")
                }
            CameraEvent.Recover ->
                when (state.status) {
                    CameraStatus.ERROR, CameraStatus.RECOVERING ->
                        state.copy(status = CameraStatus.OPENING, recoveries = state.recoveries + 1)
                    else -> state
                }
            CameraEvent.Close -> state.copy(status = CameraStatus.CLOSED, lastError = null)
            CameraEvent.Closed ->
                when (state.status) {
                    CameraStatus.CLOSED -> state
                    else -> state.copy(status = CameraStatus.CLOSED)
                }
        }
}
