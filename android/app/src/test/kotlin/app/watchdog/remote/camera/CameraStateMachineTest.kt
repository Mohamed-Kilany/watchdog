package app.watchdog.remote.camera

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CameraStateMachineTest {
    @Test
    fun `happy path reaches streaming`() {
        val machine = CameraStateMachine()
        machine.onEvent(CameraEvent.Open("0"))
        assertEquals(CameraStatus.OPENING, machine.snapshot.status)
        assertEquals("0", machine.snapshot.cameraId)

        machine.onEvent(CameraEvent.Opened)
        assertEquals(CameraStatus.OPEN, machine.snapshot.status)

        machine.onEvent(CameraEvent.Configured)
        assertEquals(CameraStatus.CONFIGURED, machine.snapshot.status)

        machine.onEvent(CameraEvent.StreamStarted)
        assertEquals(CameraStatus.STREAMING, machine.snapshot.status)
        assertNull(machine.snapshot.lastError)
    }

    @Test
    fun `error then recover reopens and counts attempts`() {
        val machine = CameraStateMachine()
        machine.onEvent(CameraEvent.Open("1"))
        machine.onEvent(CameraEvent.Failed("camera in use by another app", cameraInUse = true))

        assertEquals(CameraStatus.ERROR, machine.snapshot.status)
        assertEquals("camera in use by another app", machine.snapshot.lastError)

        machine.onEvent(CameraEvent.Recover)
        assertEquals(CameraStatus.OPENING, machine.snapshot.status)
        assertEquals(1, machine.snapshot.recoveries)

        machine.onEvent(CameraEvent.Opened)
        assertEquals(CameraStatus.OPEN, machine.snapshot.status)
    }

    @Test
    fun `disconnect moves to recovering and reopen works`() {
        val machine = CameraStateMachine()
        machine.onEvent(CameraEvent.Open("0"))
        machine.onEvent(CameraEvent.Opened)
        machine.onEvent(CameraEvent.Configured)
        machine.onEvent(CameraEvent.StreamStarted)

        machine.onEvent(CameraEvent.Disconnected)
        assertEquals(CameraStatus.RECOVERING, machine.snapshot.status)
        assertEquals("camera disconnected", machine.snapshot.lastError)

        machine.onEvent(CameraEvent.Recover)
        assertEquals(CameraStatus.OPENING, machine.snapshot.status)
        machine.onEvent(CameraEvent.Opened)
        assertEquals(CameraStatus.OPEN, machine.snapshot.status)
    }

    @Test
    fun `stream stop returns to configured`() {
        val machine = CameraStateMachine()
        machine.onEvent(CameraEvent.Open("0"))
        machine.onEvent(CameraEvent.Opened)
        machine.onEvent(CameraEvent.Configured)
        machine.onEvent(CameraEvent.StreamStarted)

        machine.onEvent(CameraEvent.StreamStopped)
        assertEquals(CameraStatus.CONFIGURED, machine.snapshot.status)
    }

    @Test
    fun `invalid events are ignored`() {
        val machine = CameraStateMachine()
        machine.onEvent(CameraEvent.Opened)
        machine.onEvent(CameraEvent.Configured)
        machine.onEvent(CameraEvent.StreamStarted)
        assertEquals(CameraStatus.CLOSED, machine.snapshot.status)

        machine.onEvent(CameraEvent.Open("0"))
        machine.onEvent(CameraEvent.StreamStarted)
        assertEquals(CameraStatus.OPENING, machine.snapshot.status)

        machine.onEvent(CameraEvent.Recover)
        assertEquals(CameraStatus.OPENING, machine.snapshot.status)
        assertEquals(0, machine.snapshot.recoveries)
    }

    @Test
    fun `close from streaming resets the state`() {
        val machine = CameraStateMachine()
        machine.onEvent(CameraEvent.Open("0"))
        machine.onEvent(CameraEvent.Opened)
        machine.onEvent(CameraEvent.Configured)
        machine.onEvent(CameraEvent.StreamStarted)

        machine.onEvent(CameraEvent.Close)
        assertEquals(CameraStatus.CLOSED, machine.snapshot.status)
        assertNull(machine.snapshot.lastError)

        machine.onEvent(CameraEvent.Closed)
        assertEquals(CameraStatus.CLOSED, machine.snapshot.status)
    }

    @Test
    fun `hundred open close cycles leave a clean state`() {
        val machine = CameraStateMachine()
        repeat(100) {
            machine.onEvent(CameraEvent.Open("0"))
            machine.onEvent(CameraEvent.Opened)
            machine.onEvent(CameraEvent.Configured)
            machine.onEvent(CameraEvent.StreamStarted)
            machine.onEvent(CameraEvent.Close)
        }
        assertEquals(CameraStatus.CLOSED, machine.snapshot.status)
        assertEquals(0, machine.snapshot.recoveries)
    }
}
