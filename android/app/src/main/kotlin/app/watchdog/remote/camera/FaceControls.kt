package app.watchdog.remote.camera

import android.graphics.Rect
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.Face
import org.json.JSONArray
import org.json.JSONObject

/**
 * M4-T08 face detection readouts. `face_detect` enables the detector and the
 * result carries `STATISTICS_FACES`; boxes are normalized so the host can draw
 * them over the viewfinder (M6-T04) and never burn them into the video.
 */
internal object FaceControls {
    /** Add `face_count` and `faces` to a result extraction. */
    fun extract(result: TotalCaptureResult, chars: CameraCharacteristics, out: MutableMap<String, Any>) {
        val faces = result.get(CaptureResult.STATISTICS_FACES) ?: return
        out["face_count"] = faces.size
        val active = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE)
        val boxes = JSONArray()
        faces.forEach { face -> boxes.put(box(face, active)) }
        out["faces"] = boxes
    }

    private fun box(face: Face, active: Rect?): JSONObject {
        val bounds = face.bounds
        val out = JSONObject().put("id", face.id).put("score", face.score)
        if (active == null) {
            return out
        }
        val values =
            FaceMath.normalize(
                bounds.left,
                bounds.top,
                bounds.right,
                bounds.bottom,
                active.left,
                active.top,
                active.width(),
                active.height(),
            ) ?: return out
        return out
            .put("x", values[0])
            .put("y", values[1])
            .put("w", values[2])
            .put("h", values[3])
    }
}
