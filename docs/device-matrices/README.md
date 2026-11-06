# Device matrices

Per-device output of the "probe everything" capability sweep (M4-T10): every Camera2
request key the device advertises is attempted with each documented value and recorded as
accepted / ignored / error, plus whether the result key reflected it.

Matrices land in M4-T10, one file per device/Android combination:
`<model>-<android>.md` (e.g. `pixel7-14.md`). Do not hand-edit; regenerate with
`watchdog device capabilities --probe --json` once M4 lands.
