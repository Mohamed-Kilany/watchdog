package app.watchdog.remote

import android.hardware.camera2.CaptureRequest
import app.watchdog.remote.camera.ControlSource
import app.watchdog.remote.camera.ControlStateStore
import app.watchdog.remote.protocol.JsonMessages
import org.json.JSONObject

/**
 * Cross-control implications applied atomically with a `set_controls` batch
 * (M4-T04/T05, extended by M4-T06).
 *
 * Keeping this out of [CaptureService] leaves the service as a transport and
 * camera-lifecycle coordinator while the implication rules stay together and
 * testable. All state is confined to the command thread that calls [apply].
 */
class ControlImplications {
    private var afModeBeforeLock: Int? = null
    private var lastAfMode: Int? = null

    /** Remember the AF mode reported by the device for later lock restore. */
    fun onCaptureResult(id: String, value: Any) {
        if (id == "af_mode") {
            lastAfMode = (value as? Number)?.toInt()
        }
    }

    /** Forget per-camera state; called when the camera closes. */
    fun reset() {
        afModeBeforeLock = null
        lastAfMode = null
    }

    /**
     * Apply the implications for one accepted batch: manual sensor implies AE
     * off, manual focus distance implies AF off and clears AF lock, and an AF
     * lock drives a one-shot scan (restoring the previous mode on unlock).
     */
    fun apply(
        store: ControlStateStore,
        toApply: MutableMap<String, Any>,
        results: MutableList<JSONObject>,
        triggers: MutableMap<String, Any>,
        manualSensor: Boolean,
    ) {
        if (manualSensor) {
            implyAeOff(store, toApply, results)
        }
        if ("focus_distance" in toApply) {
            implyAfOff(store, toApply, results)
        }
        if ("af_lock" in toApply) {
            implyAfLock(store, toApply, results, triggers)
        }
        if ("awb_gains" in toApply || "color_transform" in toApply) {
            implyAwbOff(store, toApply, results)
        }
    }

    /**
     * Manual ISO/exposure only take effect with AE off. Apply it atomically in
     * the same batch and report it so the host sees the effective state.
     */
    private fun implyAeOff(
        store: ControlStateStore,
        toApply: MutableMap<String, Any>,
        results: MutableList<JSONObject>,
    ) {
        if ("ae_mode" in toApply || !store.isSupported("ae_mode")) {
            return
        }
        val validation = store.setRequested("ae_mode", CaptureRequest.CONTROL_AE_MODE_OFF, ControlSource.HOST)
        if (validation.ok) {
            validation.applied?.let { toApply["ae_mode"] = it }
            results += JsonMessages.controlResult("ae_mode", true, applied = validation.applied)
        }
    }

    /** Manual focus distance only takes effect with AF off (M4-T05). */
    private fun implyAfOff(
        store: ControlStateStore,
        toApply: MutableMap<String, Any>,
        results: MutableList<JSONObject>,
    ) {
        if (!store.isSupported("af_mode")) {
            return
        }
        setAfMode(store, toApply, results, CaptureRequest.CONTROL_AF_MODE_OFF)
        if (store.requestedValues()["af_lock"] == true) {
            afModeBeforeLock = null
            val unlock = store.setRequested("af_lock", false, ControlSource.HOST)
            if (unlock.ok) {
                unlock.applied?.let { toApply["af_lock"] = it }
                results += JsonMessages.controlResult("af_lock", true, applied = unlock.applied)
            }
        }
    }

    /**
     * `af_lock = true` switches to one-shot AF and triggers a scan; unlocking
     * restores the AF mode that was active before the lock. The reported lock
     * state comes from `CONTROL_AF_STATE` (`focused_locked`/`not_focused_locked`).
     */
    private fun implyAfLock(
        store: ControlStateStore,
        toApply: MutableMap<String, Any>,
        results: MutableList<JSONObject>,
        triggers: MutableMap<String, Any>,
    ) {
        if ("focus_distance" in toApply || !store.isSupported("af_mode")) {
            return
        }
        if (toApply["af_lock"] == true) {
            if (afModeBeforeLock == null) {
                afModeBeforeLock =
                    (store.requestedValues()["af_mode"] as? Number)?.toInt() ?: lastAfMode
            }
            if ("af_mode" !in toApply) {
                setAfMode(store, toApply, results, CaptureRequest.CONTROL_AF_MODE_AUTO)
            }
            triggers["af_trigger"] = "start"
            return
        }
        val restore = afModeBeforeLock
        afModeBeforeLock = null
        if (restore != null && "af_mode" !in toApply) {
            setAfMode(store, toApply, results, restore)
        }
        triggers["af_trigger"] = "cancel"
    }

    /**
     * Manual gains or a colour matrix only take effect with AWB off; the
     * native lock is cleared so it cannot fight the manual white point.
     */
    private fun implyAwbOff(
        store: ControlStateStore,
        toApply: MutableMap<String, Any>,
        results: MutableList<JSONObject>,
    ) {
        if ("awb_mode" !in toApply && store.isSupported("awb_mode")) {
            val validation = store.setRequested("awb_mode", CaptureRequest.CONTROL_AWB_MODE_OFF, ControlSource.HOST)
            if (validation.ok) {
                validation.applied?.let { toApply["awb_mode"] = it }
                results += JsonMessages.controlResult("awb_mode", true, applied = validation.applied)
            }
        }
        if (store.requestedValues()["awb_lock"] == true) {
            val unlock = store.setRequested("awb_lock", false, ControlSource.HOST)
            if (unlock.ok) {
                unlock.applied?.let { toApply["awb_lock"] = it }
                results += JsonMessages.controlResult("awb_lock", true, applied = unlock.applied)
            }
        }
    }

    private fun setAfMode(
        store: ControlStateStore,
        toApply: MutableMap<String, Any>,
        results: MutableList<JSONObject>,
        mode: Int,
    ) {
        val validation = store.setRequested("af_mode", mode, ControlSource.HOST)
        if (validation.ok) {
            validation.applied?.let { toApply["af_mode"] = it }
            results += JsonMessages.controlResult("af_mode", true, applied = validation.applied)
        }
    }
}
