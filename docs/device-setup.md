# Device setup

How to connect a phone to watchdog — design targets are the **USB debugging** flow and
**Android 11+ wireless debugging** (pairing). No system `adb` is used or required.

## 1. Enable USB debugging on the phone

1. Settings → About phone → tap **Build number** 7 times (enables Developer options).
2. Settings → System → Developer options → enable **USB debugging**.
3. Connect USB; when the phone shows "Allow USB debugging?", tap **Allow**
   (tick "Always allow from this computer" to make it permanent).
   - This is the only required tap.
4. On the host: `watchdog device list`

First-time watchdog authorization uses **its own** RSA key pair stored in
`<data_dir>/keys/adbkey` — never `~/.android`.

## 2. Per-OS notes

### Linux

- udev rules are required for non-root USB access. Install them:
  `sudo watchdog device udev-rules --write`
  or copy the output of `watchdog device udev-rules` to
  `/etc/udev/rules.d/51-android-watchdog.rules`, then
  `sudo udevadm control --reload-rules && sudo udevadm trigger`.
- Add yourself to `plugdev` if using group-based rules.
- `watchdog device usb-doctor` explains remaining permission problems.

### macOS

- No driver needed for most devices (uses IOKit / libusb backend).
- If the device is "not authorized", approve the debugging prompt on the phone.

### Windows

- Many Android devices need the **WinUSB** driver bound via
  [Zadig](https://zadig.akeo.ie/) ("Install WCID Driver" / replace driver for the
  "Android ADB Interface", or use libwdi). Some vendors ship a signed WinUSB-based
  driver — then no Zadig step is needed.
- Do not install Google's "Android ADB Interface" *and* watchdog's WinUSB binding
  confused: watchdog talks raw USB; the system driver is unnecessary for it.
- If Device Manager shows "Unknown device", pick WinUSB in Zadig for interface MI_01
  (the adb interface, class ff/42/01).

## 3. Wireless debugging (Android 11+, pairing)

1. Phone: Developer options → Wireless debugging → **Pair device with pairing code**.
2. Host: `watchdog device pair <ip>:<pairing_port> <code>`
   (watchdog speaks the SPAKE2 + TLS pairing flow natively; no system adb involved).
3. Then connect: `watchdog device connect <ip>:<connect_port>` (the main wireless
   debugging port, shown on the same screen).
4. `watchdog device provision --serial <ip>:<connect_port>`

## 4. `adb tcpip`-style connect (USB → LAN)

With the phone on USB: enable wireless debugging or use `watchdog device forward`-style
commands once implemented (M2). Details land with M2-T05.

## 5. Zero-touch provisioning — what it automates

`watchdog device provision` performs (each step reported pass/fail/skipped, idempotent, resumable):

1. detect authorized device; read props (SDK, ABI, model, OEM);
2. install/update the embedded APK (signature mismatch → explicit, documented uninstall path);
3. `pm grant` runtime permissions (camera, microphone, notifications, media) + appops;
4. battery optimization exemption (`dumpsys deviceidle whitelist`), standby bucket, background restriction off;
5. screen wake / keyguard dismiss where possible; keep-awake-while-plugged settings
   with **original values recorded and reverted on `watchdog device restore`**;
6. launch the app with a per-session token;
7. verify first frame arrives.

## 6. OEM quirks compatibility table

| OEM / OS | Notes | Status |
|---|---|---|
| Google Pixel (stock AOSP) | reference device; all steps automated | verified (see docs/verification/) |
| Samsung One UI | "Auto blocker" may block unknown installs — allow once; battery "Put unused apps to sleep" must not apply to the app (provision sets it) | partial; verify per device |
| Xiaomi MIUI / HyperOS | USB debugging (Security settings) toggle also required; MIUI kills background apps aggressively — provision sets Autostart + battery exemption | partial |
| Huawei EMUI | no Google services needed; wireless debugging absent on some EMUI — use USB | unverified |
| OnePlus OxygenOS | near-stock; works like Pixel | unverified |
| Android 8–10 | no wireless-debugging pairing (Android 11+); use USB | supported for USB |

If a step cannot be automated on a given device, provisioning **fails with the exact
manual step** and records it in the UI/CLI; this table is updated from verification runs.

## 7. Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `device list` empty | USB driver/udev missing | Linux: `sudo watchdog device udev-rules --write`; Windows: Zadig → WinUSB |
| Device shows as `unauthorized` | debugging prompt not accepted | tap Allow on the phone (untick/retick "always allow") |
| Device shows as `offline` | stale session / flaky cable | replug; try another cable (charge-only cables can't do ADB) |
| `pair` fails with code error | pairing port/code mismatch | re-open "Pair device with pairing code"; code is single-use |
| Install blocked | unknown-sources policy / signature mismatch | allow once on device; signature mismatch → `watchdog device uninstall` then provision |
| App keeps getting killed (Xiaomi etc.) | aggressive OEM battery management | provision sets exemptions; verify with `watchdog device status` |
| LAN connect drops | IP changed (DHCP) | re-run `watchdog device discover`; reserve DHCP lease |
| Nothing works | a competing adb server on 5037 | irrelevant — watchdog does not use 5037; see docs/security.md |

## 8. Security notes

- The watchdog ADB private key lives in `<data_dir>/keys/`; treat it like an SSH key.
  Back it up to re-use "always allow" authorizations; delete it to revoke them everywhere.
- Anyone with the key + network access to the device can run shell commands. Do not
  commit it; file mode 0600.
- Wireless debugging exposes more than USB — keep the pairing code private, disable
  wireless debugging when done, or restrict to a trusted VLAN.
