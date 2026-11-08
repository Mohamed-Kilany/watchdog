plugins {
    alias(libs.plugins.android.application)
}

/**
 * Version name derived from the git tag by CI (`WATCHDOG_APP_VERSION`).
 * Falls back to a dev version for local builds. The tag is the single source
 * of truth so host, APK and release notes always agree.
 */
fun appVersionName(): String {
    val raw = System.getenv("WATCHDOG_APP_VERSION")?.trim().orEmpty()
    return raw.removePrefix("v").ifEmpty { "0.1.0-dev" }
}

/**
 * Deterministic versionCode: `major * 1_000_000 + minor * 1_000 + patch`.
 * Build metadata (everything after `-` or `+`) does not affect the code.
 */
fun appVersionCode(name: String): Int {
    val core = name.substringBefore('-').substringBefore('+')
    val parts = core.split('.').mapNotNull { it.toIntOrNull() }
    val major = parts.getOrElse(0) { 0 }
    val minor = parts.getOrElse(1) { 0 }
    val patch = parts.getOrElse(2) { 0 }
    return major * 1_000_000 + minor * 1_000 + patch
}

val versionName = appVersionName()
val versionCode = appVersionCode(versionName)

val releaseKeystore = System.getenv("WATCHDOG_KEYSTORE_FILE")

android {
    namespace = "app.watchdog.remote"
    // API 37 is published as platform 37.0 (minor API level 0); the new DSL
    // resolves platforms/android-37.0 instead of the non-existent android-37.
    compileSdk {
        version =
            release(37) {
                minorApiLevel = 0
            }
    }

    defaultConfig {
        applicationId = "app.watchdog.remote"
        minSdk = 26
        targetSdk = 37
        versionCode = versionCode
        versionName = versionName
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("WATCHDOG_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("WATCHDOG_KEY_ALIAS") ?: "watchdog"
                keyPassword = System.getenv("WATCHDOG_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            // CI provides a keystore; local release builds fall back to the
            // debug key so `assembleRelease` always yields an installable APK.
            signingConfig =
                if (releaseKeystore != null) {
                    signingConfigs.getByName("release")
                } else {
                    signingConfigs.getByName("debug")
                }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        viewBinding = true
        buildConfig = true
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.appcompat)
    implementation(libs.material)
    implementation(libs.androidx.constraintlayout)
    testImplementation(libs.junit)
}
