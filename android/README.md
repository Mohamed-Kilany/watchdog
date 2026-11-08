# watchdog remote (Android app)

Kotlin app embedded in watchdog releases. It owns the camera, runs a
foreground service, and serves frames/control to the host over an abstract
local socket.

## Toolchain

| Item | Version | Notes |
|---|---|---|
| Gradle | 9.6.0 | committed wrapper (`./gradlew`) |
| Android Gradle plugin | 9.5.0 | built-in Kotlin 2.2.10 |
| JDK | 17 | CI uses Temurin 17 |
| compileSdk / targetSdk | 37 (platform 37.0) | Android 17 (latest stable at M3); AGP resolves the minor API level 0 |
| minSdk | 26 | Android 8.0 |
| Application id | `app.watchdog.remote` | change only via ADR |

## Build

CI (`.github/workflows/ci.yml`, job `android`) runs ktlint, detekt and:

```sh
cd android
./gradlew assembleRelease
```

The release APK is signed with the CI keystore (or with a secret-provided
keystore when `WATCHDOG_KEYSTORE_FILE` etc. are set) and uploaded as the
`watchdog-android-apk` artifact together with `android/build/apk-info.json`
(version, SHA-256, signer certificate digest, commit, run id).

Local release builds fall back to the debug keystore so they are always
installable. Never ship a debug-signed build.

## Versioning

`WATCHDOG_APP_VERSION` is derived in CI from the git tag (`v1.2.3` → `1.2.3`)
or `0.1.0-dev+<sha>` on untagged commits. `versionCode` is deterministic:
`major * 1_000_000 + minor * 1_000 + patch`. The host embeds the same values
from `apk-info.json` (M3-T12).

## Layout

```
app/src/main/kotlin/app/watchdog/remote/   Kotlin sources
app/src/main/res/                          layouts, strings, theme
app/src/test/                              JVM unit tests (M3-T11)
app/src/androidTest/                       instrumented tests (M3-T11)
```
