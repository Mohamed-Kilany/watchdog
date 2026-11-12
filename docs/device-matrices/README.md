# Device matrices

Per-device output of the "probe everything" capability sweep (M4-T10): every Camera2
request key the device advertises is attempted with each documented value and recorded as
accepted / ignored / error, plus whether the result key reflected it.

One file per device/Android combination: `<model>-<android>.md`, with a `-camera-<id>`
suffix when the device exposes more than one camera. Do not hand-edit; regenerate with:

```bash
watchdog device probe --serial SERIAL --camera 0 --output docs/device-matrices/<model>-<android>.md
```

Use `--json` for the raw report and `--key android.control.aeMode` to probe one key.

Current matrices:

- [SM-A515F, Android 13, camera 0 (back)](sm-a515f-android-13.md)
- [SM-A515F, Android 13, camera 1 (front)](sm-a515f-android-13-camera-1.md)
- [SM-A515F, Android 13, camera 2 (back)](sm-a515f-android-13-camera-2.md)
- [SM-A515F, Android 13, camera 3 (front)](sm-a515f-android-13-camera-3.md)

See the [M4-T10 verification record](../verification/2026-11-12-m4-probe.md) for the run
details and known ignored values.
